use super::index::valid_uri;
use md5::Md5;
use quick_xml::{Reader, escape::escape, events::Event};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use url::Url;

const MAX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_CACHE_METADATA: u64 = 65_536;
const METADATA_AGE: i64 = 24 * 60 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FetchError {
    NotFound,
    AccessDenied,
    Offline,
    Timeout,
    TooLarge,
    InvalidSource,
    Checksum,
    Redirect,
    Io,
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt.write_str(match self {
            Self::NotFound => "not found",
            Self::AccessDenied => "access denied",
            Self::Offline => "no verified offline copy",
            Self::Timeout => "request timed out",
            Self::TooLarge => "source exceeds the byte limit",
            Self::InvalidSource => "source is not valid UTF-8",
            Self::Checksum => "published MD5 does not match the bytes",
            Self::Redirect => "invalid redirect or redirect limit reached",
            Self::Io => "source could not be read",
        })
    }
}

impl From<io::Error> for FetchError {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::NotFound,
            io::ErrorKind::PermissionDenied => Self::AccessDenied,
            io::ErrorKind::TimedOut => Self::Timeout,
            _ => Self::Io,
        }
    }
}

pub(super) struct Payload {
    pub text: String,
}

pub(super) struct Loader {
    client: Option<ureq::Agent>,
    offline: bool,
    cache: Option<PathBuf>,
    pub cache_write_failed: bool,
}

impl Loader {
    pub fn new(offline: bool, cache: Option<PathBuf>) -> Self {
        Self {
            client: None,
            offline,
            cache,
            cache_write_failed: false,
        }
    }

    pub fn get(
        &mut self,
        uri: &Url,
        md5: Option<&str>,
        model: bool,
    ) -> Result<Payload, FetchError> {
        if uri.scheme() == "file" {
            // The repository offline port only replays verified HTTP content.
            // Explicit and neighbouring files are supplied by the local host.
            if self.offline {
                return Err(FetchError::Offline);
            }
            let payload = read_local(uri)?;
            verify_md5(payload.text.as_bytes(), md5)?;
            return Ok(payload);
        }
        let max_age = time::Duration::seconds(if model {
            7 * METADATA_AGE
        } else {
            METADATA_AGE
        });
        let now = OffsetDateTime::now_utc();
        if let Some(cache) = &self.cache
            && let Some((bytes, downloaded)) = lookup(cache, uri, self.offline)
            && (self.offline
                || downloaded.is_some_and(|date| date <= now && now - date <= max_age)
                    && verify_md5(&bytes, md5).is_ok())
        {
            verify_md5(&bytes, md5)?;
            return decode(bytes);
        }
        if self.offline {
            return Err(FetchError::Offline);
        }
        let client = self.client.get_or_insert_with(|| {
            ureq::Agent::new_with_config(
                ureq::config::Config::builder()
                    .max_redirects(0)
                    .max_redirects_will_error(false)
                    .http_status_as_error(false)
                    .timeout_connect(Some(Duration::from_secs(15)))
                    .timeout_per_read(Some(Duration::from_secs(40)))
                    .build(),
            )
        });
        let (bytes, response_uri, reusable) = match download(client, uri) {
            Err(FetchError::NotFound) => {
                if let Some(cache) = &self.cache {
                    remove_metadata(cache, uri);
                }
                return Err(FetchError::NotFound);
            }
            result => result?,
        };
        // Even an invalid newly downloaded representation revokes a prior
        // reusable URL when the server explicitly sends no-store.
        if !reusable && let Some(cache) = &self.cache {
            remove_metadata(cache, uri);
        }
        verify_md5(&bytes, md5)?;
        let payload = decode(bytes)?;
        if reusable && let Some(cache) = &self.cache {
            self.cache_write_failed |=
                publish(cache, uri, &response_uri, payload.text.as_bytes()).is_err();
        }
        Ok(payload)
    }
}

pub(super) fn read_local(uri: &Url) -> Result<Payload, FetchError> {
    let path = uri.to_file_path().map_err(|_| FetchError::Io)?;
    decode(read_file(&path, MAX_BYTES)?)
}

pub(super) fn verify_md5(bytes: &[u8], expected: Option<&str>) -> Result<(), FetchError> {
    if expected
        .is_some_and(|value| !value.eq_ignore_ascii_case(&format!("{:x}", Md5::digest(bytes))))
    {
        Err(FetchError::Checksum)
    } else {
        Ok(())
    }
}

fn decode(bytes: Vec<u8>) -> Result<Payload, FetchError> {
    let text = String::from_utf8(bytes).map_err(|_| FetchError::InvalidSource)?;
    Ok(Payload { text })
}

fn read_file(path: &Path, limit: u64) -> Result<Vec<u8>, FetchError> {
    if !fs::metadata(path)?.is_file() {
        return Err(FetchError::Io);
    }
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(FetchError::Io);
    }
    read_bytes(file, limit)
}

fn read_bytes(input: impl Read, limit: u64) -> Result<Vec<u8>, FetchError> {
    let mut bytes = Vec::new();
    input.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(FetchError::TooLarge);
    }
    Ok(bytes)
}

fn download(client: &ureq::Agent, uri: &Url) -> Result<(Vec<u8>, Url, bool), FetchError> {
    let mut current = uri.clone();
    let mut reusable = true;
    for redirects in 0..=5 {
        let mut response = client
            .get(current.as_str())
            .header("Accept-Encoding", "identity")
            .call()
            .map_err(|error| match error {
                ureq::Error::Timeout(_) => FetchError::Timeout,
                _ => FetchError::Io,
            })?;
        reusable &= !response
            .headers()
            .get_all("Cache-Control")
            .iter()
            .any(|value| {
                value.to_str().is_ok_and(|value| {
                    value
                        .split(',')
                        .any(|v| v.trim().eq_ignore_ascii_case("no-store"))
                })
            });
        match response.status().as_u16() {
            200 => {
                if response
                    .headers()
                    .get_all("Content-Encoding")
                    .iter()
                    .any(|value| {
                        !value
                            .to_str()
                            .is_ok_and(|value| value.eq_ignore_ascii_case("identity"))
                    })
                {
                    return Err(FetchError::Io);
                }
                let bytes = read_bytes(response.body_mut().as_reader(), MAX_BYTES)?;
                return Ok((bytes, current, reusable));
            }
            304 => return Err(FetchError::Io),
            300..=399 if redirects < 5 => {
                let location = response
                    .headers()
                    .get("Location")
                    .and_then(|value| value.to_str().ok())
                    .ok_or(FetchError::Redirect)?;
                let next = current.join(location).map_err(|_| FetchError::Redirect)?;
                if !valid_uri(&next) || !matches!(next.scheme(), "http" | "https") {
                    return Err(FetchError::Redirect);
                }
                current = next;
            }
            300..=399 => return Err(FetchError::Redirect),
            404 => return Err(FetchError::NotFound),
            401 | 403 => return Err(FetchError::AccessDenied),
            _ => return Err(FetchError::Io),
        }
    }
    Err(FetchError::Redirect)
}

// These two names and the XML record match interlis-java's on-disk cache.
// The URL digest locates metadata. The content digest verifies offline bytes.
fn metadata_path(cache: &Path, uri: &Url, legacy: bool) -> PathBuf {
    let key = Sha256::digest(uri.as_str().as_bytes());
    cache
        .join(if legacy { "metadata-v1" } else { "metadata-v2" })
        .join(format!("{key:x}.xml"))
}

fn remove_metadata(cache: &Path, uri: &Url) {
    for legacy in [false, true] {
        let _ = fs::remove_file(metadata_path(cache, uri, legacy));
    }
}

fn lookup(cache: &Path, uri: &Url, offline: bool) -> Option<(Vec<u8>, Option<OffsetDateTime>)> {
    let current = read_file(&metadata_path(cache, uri, false), MAX_CACHE_METADATA);
    let legacy = offline && matches!(current, Err(FetchError::NotFound));
    let xml = if legacy {
        read_file(&metadata_path(cache, uri, true), MAX_CACHE_METADATA).ok()?
    } else {
        current.ok()?
    };
    let mut reader = Reader::from_reader(xml.as_slice());
    reader.config_mut().trim_text(true);
    reader.config_mut().expand_empty_elements = true;
    let mut seen_root = false;
    let mut closed = false;
    let mut fields = std::collections::BTreeMap::new();
    loop {
        match reader.read_event().ok()? {
            Event::Decl(_) if !seen_root => {}
            Event::Text(text) if text.as_ref().is_empty() => {}
            Event::Start(element) if !seen_root && element.name().as_ref() == b"interlis-cache" => {
                seen_root = true;
                for attr in element.attributes() {
                    let attr = attr.ok()?;
                    let name = attr.key.as_ref();
                    if !matches!(
                        name,
                        b"version" | b"source" | b"response-source" | b"sha256"
                    ) && !(legacy && matches!(name, b"etag" | b"last-modified"))
                        && !(!legacy && name == b"downloaded-at")
                    {
                        return None;
                    }
                    fields.insert(
                        name.to_vec(),
                        attr.decode_and_unescape_value(reader.decoder())
                            .ok()?
                            .into_owned(),
                    );
                }
            }
            Event::End(_) if seen_root && !closed => closed = true,
            Event::Eof if closed => break,
            _ => return None,
        }
    }
    let field = |key: &[u8]| fields.get(key).map(String::as_str);
    if field(b"version")? != if legacy { "1" } else { "2" }
        || field(b"source")? != uri.as_str()
        || !valid_uri(&Url::parse(field(b"response-source")?).ok()?)
    {
        return None;
    }
    let digest = field(b"sha256")?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let date = if legacy {
        None
    } else {
        let date = OffsetDateTime::parse(field(b"downloaded-at")?, &Rfc3339).ok()?;
        (date != OffsetDateTime::UNIX_EPOCH).then_some(date)
    };
    let bytes = read_file(&cache.join(digest.to_ascii_lowercase()), MAX_BYTES).ok()?;
    digest
        .eq_ignore_ascii_case(&format!("{:x}", Sha256::digest(&bytes)))
        .then_some((bytes, date))
}

fn publish(cache: &Path, uri: &Url, response: &Url, bytes: &[u8]) -> io::Result<()> {
    let date = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(io::Error::other)?;
    let digest = format!("{:x}", Sha256::digest(bytes));
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><interlis-cache version=\"2\" source=\"{}\" response-source=\"{}\" sha256=\"{digest}\" downloaded-at=\"{date}\"/>",
        escape(uri.as_str()),
        escape(response.as_str()),
    );
    if xml.len() as u64 > MAX_CACHE_METADATA {
        return Err(io::Error::other("cache metadata exceeds the byte limit"));
    }
    atomic_write(&cache.join(digest), bytes)?;
    atomic_write(&metadata_path(cache, uri, false), xml.as_bytes())
}

fn atomic_write(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let directory = target.parent().expect("cache entry has a directory");
    fs::create_dir_all(directory)?;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    file.write_all(bytes)?;
    file.persist(target)?;
    Ok(())
}
