use super::load::{FetchError, Loader, Payload};
use crate::{LanguageVersion, limits::WorkBudget};
use quick_xml::{Reader, events::Event};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use url::Url;

const INDEX: &str = "ilimodels.xml";

#[derive(Clone)]
pub(super) struct Repository {
    base: Url,
    index: Url,
}

impl Repository {
    pub fn parse(value: &str) -> Result<Self, ()> {
        if value.trim().is_empty() {
            return Err(());
        }
        let uri = if ["http:", "https:", "file:"].iter().any(|prefix| {
            value
                .get(..prefix.len())
                .is_some_and(|s| s.eq_ignore_ascii_case(prefix))
        }) {
            Url::parse(value).map_err(|_| ())?
        } else {
            let path = std::path::absolute(value).map_err(|_| ())?;
            let uri = Url::from_file_path(path)?;
            Url::parse(uri.as_str()).map_err(|_| ())?
        };
        if !valid_uri(&uri) {
            return Err(());
        }
        if uri.path().to_ascii_lowercase().ends_with(".xml") {
            return Ok(Self {
                base: uri.join(".").map_err(|_| ())?,
                index: uri,
            });
        }
        Self::directory(uri)
    }

    fn directory(mut base: Url) -> Result<Self, ()> {
        if !valid_uri(&base) || base.query().is_some() {
            return Err(());
        }
        if !base.path().ends_with('/') {
            base.path_segments_mut()?.push("");
        }
        let index = base.join(INDEX).map_err(|_| ())?;
        Ok(Self { base, index })
    }

    fn site_identity(&self) -> Url {
        self.base
            .to_file_path()
            .ok()
            .and_then(|path| path.canonicalize().ok())
            .and_then(|path| Url::from_directory_path(path).ok())
            .unwrap_or_else(|| self.base.clone())
    }
}

pub(super) fn valid_uri(uri: &Url) -> bool {
    matches!(uri.scheme(), "http" | "https" | "file")
        && uri.username().is_empty()
        && uri.password().is_none()
        && uri.fragment().is_none()
        && (uri.scheme() != "file" || uri.to_file_path().is_ok())
}

pub(super) fn safe_uri(uri: &Url) -> String {
    let mut public = uri.clone();
    public.set_query(None);
    public.set_fragment(None);
    public.to_string()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ModelEntry {
    pub uri: Url,
    pub edition: Option<LanguageVersion>,
    pub version: String,
    pub md5: Option<String>,
    pub priority: usize,
    precursor: Option<String>,
    browse_only: bool,
}

type Index = BTreeMap<String, Vec<ModelEntry>>;

#[derive(Default)]
struct Site {
    parents: Vec<Repository>,
    children: Vec<Repository>,
}

#[derive(Debug)]
struct Element {
    name: String,
    text: String,
}

// Read only the element path and its direct character data. Retain neither
// the XML tree nor unrelated metadata from large repository indexes.
fn xml_fields(text: &str, mut close: impl FnMut(&[Element]) -> Result<(), ()>) -> Result<(), ()> {
    use quick_xml::escape::resolve_predefined_entity;
    let mut reader = Reader::from_str(text);
    reader.config_mut().expand_empty_elements = true;
    reader.config_mut().check_comments = true;
    let mut stack: Vec<Element> = Vec::new();
    let mut root = false;
    loop {
        match reader.read_event().map_err(|_| ())? {
            Event::Start(start) => {
                if stack.len() >= 128 || (stack.is_empty() && std::mem::replace(&mut root, true)) {
                    return Err(());
                }
                for attribute in start.attributes() {
                    attribute.map_err(|_| ())?;
                }
                stack.push(Element {
                    name: std::str::from_utf8(start.local_name().as_ref())
                        .map_err(|_| ())?
                        .into(),
                    text: String::new(),
                });
            }
            Event::End(_) => {
                close(&stack)?;
                stack.pop().ok_or(())?;
            }
            Event::Text(value) => {
                let value = value.xml10_content().map_err(|_| ())?;
                if let Some(active) = stack.last_mut() {
                    active.text.push_str(&value);
                } else if !value.trim().is_empty() {
                    return Err(());
                }
            }
            Event::CData(value) => stack
                .last_mut()
                .ok_or(())?
                .text
                .push_str(&value.xml10_content().map_err(|_| ())?),
            Event::GeneralRef(value) => {
                let active = stack.last_mut().ok_or(())?;
                if let Some(character) = value.resolve_char_ref().map_err(|_| ())? {
                    active.text.push(character);
                } else {
                    active.text.push_str(
                        resolve_predefined_entity(&value.decode().map_err(|_| ())?).ok_or(())?,
                    );
                }
            }
            Event::DocType(_) => return Err(()),
            Event::Eof => {
                return if root && stack.is_empty() {
                    Ok(())
                } else {
                    Err(())
                };
            }
            _ => {}
        }
    }
}

fn model_element(s: &str) -> bool {
    s == "ModelMetadata" || s.ends_with(".ModelMetadata")
}

fn file_uri(base: &Url, value: &str) -> Result<Url, ()> {
    let uri = base.join(value).map_err(|_| ())?;
    // URL paths stay escaped. Path separators must have the same meaning to
    // the URL parser and the file/server receiving the request.
    let escaped = value
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let encoded_dot = escaped.split('/').any(|part| {
        matches!(part.replace("%2e", ".").as_str(), "." | "..") && part.contains("%2e")
    });
    if !valid_uri(&uri)
        || base.scheme() != uri.scheme()
        || base.authority() != uri.authority()
        || !uri.path().starts_with(base.path())
        || escaped.contains("%2f")
        || escaped.contains("%5c")
        || encoded_dot
    {
        return Err(());
    }
    Ok(uri)
}

fn read_index(text: &str, base: &Url) -> Result<Index, ()> {
    let mut profile = false;
    let mut index = Index::new();
    let mut fields: HashMap<String, String> = HashMap::new();
    xml_fields(text, |path| {
        let field = path.last().ok_or(())?;
        match field.name.as_str() {
            "RepositoryIndex"
            | "IliRepository09.RepositoryIndex"
            | "IliRepository20.RepositoryIndex" => profile = true,
            name if model_element(name) => {
                let mut take = |key| fields.remove(key).filter(|value| !value.trim().is_empty());
                let uri = file_uri(base, take("File").ok_or(())?.trim())?;
                let edition = match take("SchemaLanguage").ok_or(())?.trim() {
                    "ili2_3" => Some(LanguageVersion::V23),
                    "ili2_4" => Some(LanguageVersion::V24),
                    _ => None,
                };
                let name = take("Name").ok_or(())?.trim().to_owned();
                let version = take("Version").ok_or(())?;
                let precursor = take("precursorVersion");
                let md5 = take("md5").map(|v| v.trim().to_owned());
                let browse_only =
                    take("browseOnly").is_some_and(|v| v.trim().eq_ignore_ascii_case("true"));
                index.entry(name).or_default().push(ModelEntry {
                    uri,
                    edition,
                    version,
                    precursor,
                    md5,
                    priority: 0,
                    browse_only,
                });
                fields.clear();
            }
            _ if path.len() > 1 && model_element(&path[path.len() - 2].name) => {
                fields
                    .entry(field.name.clone())
                    .or_insert_with(|| field.text.clone());
            }
            _ => {}
        }
        Ok(())
    })?;
    if index.is_empty() && !profile {
        Err(())
    } else {
        Ok(index)
    }
}

fn read_site(text: &str, base: &Url) -> Result<Site, ()> {
    let mut site = Site::default();
    let mut found = false;
    xml_fields(text, |path| {
        let field = path.last().ok_or(())?;
        if matches!(field.name.as_str(), "Site" | "IliSite09.SiteMetadata.Site") {
            found = true;
        }
        if field.name == "value"
            && !field.text.trim().is_empty()
            && let Some(container) = path
                .iter()
                .rev()
                .find(|e| matches!(e.name.as_str(), "parentSite" | "subsidiarySite"))
        {
            let uri = base.join(field.text.trim()).map_err(|_| ())?;
            if base.scheme() != "file" && uri.scheme() == "file" {
                return Err(());
            }
            let repository = Repository::directory(uri)?;
            if container.name == "parentSite" {
                site.parents.push(repository);
            } else {
                site.children.push(repository);
            }
        }
        Ok(())
    })?;
    if found { Ok(site) } else { Err(()) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SearchError {
    NotFound,
    Version,
    Ambiguous,
    Index,
    Load(FetchError),
    Limit,
    Reported,
}

#[derive(Clone, Copy)]
pub(super) struct Query<'a> {
    pub name: &'a str,
    pub edition: Option<LanguageVersion>,
    pub version: Option<&'a str>,
}

pub(super) struct Search {
    pub loader: Loader,
    configured: Vec<Repository>,
    indexes: HashMap<Url, Result<Index, SearchError>>,
    sites: HashMap<Url, Site>,
    priorities: HashMap<Url, usize>,
    pub warnings: Vec<String>,
    remaining_warnings: usize,
    work: WorkBudget,
}

#[derive(Default)]
struct Visits {
    bases: HashSet<Url>,
    parents: VecDeque<Repository>,
    children: VecDeque<Repository>,
    failure: Option<SearchError>,
    other_version: bool,
}

impl Search {
    pub fn new(configured: Vec<Repository>, loader: Loader, limits: &crate::Limits) -> Self {
        Self {
            loader,
            configured,
            indexes: HashMap::new(),
            sites: HashMap::new(),
            priorities: HashMap::new(),
            warnings: Vec::new(),
            remaining_warnings: limits.max_diagnostics as usize,
            work: WorkBudget::new(limits),
        }
    }

    pub fn charge(&mut self, amount: usize) -> Result<(), SearchError> {
        self.work
            .charge(u32::try_from(amount).map_err(|_| SearchError::Limit)?)
            .map_err(|_| SearchError::Limit)
    }

    fn warn(&mut self, message: String) -> Result<(), SearchError> {
        self.remaining_warnings = self
            .remaining_warnings
            .checked_sub(1)
            .ok_or(SearchError::Limit)?;
        self.warnings.push(message);
        Ok(())
    }

    pub fn find(&mut self, query: Query<'_>) -> Result<ModelEntry, SearchError> {
        let mut visits = Visits::default();
        let mut configured = Vec::new();
        let mut indexes = HashSet::new();
        for repo in self.configured.clone() {
            self.charge(1)?;
            if visits.bases.insert(repo.site_identity()) {
                configured.push(repo.clone());
            }
            if indexes.insert(repo.index.clone())
                && let Some(model) = self.visit(&repo, query, &mut visits)?
            {
                return Ok(model);
            }
        }
        for repo in configured {
            self.add_links(&repo, &mut visits, true)?;
        }
        loop {
            let (next, child) = if let Some(repo) = visits.parents.pop_front() {
                (repo, false)
            } else if let Some(repo) = visits.children.pop_front() {
                (repo, true)
            } else {
                break;
            };
            self.charge(1)?;
            if visits.bases.insert(next.site_identity()) {
                if let Some(model) = self.visit(&next, query, &mut visits)? {
                    return Ok(model);
                }
                self.add_links(&next, &mut visits, child)?;
            }
        }
        Err(visits.failure.unwrap_or(if visits.other_version {
            SearchError::Version
        } else {
            SearchError::NotFound
        }))
    }

    fn payload(&mut self, uri: &Url) -> Result<Payload, SearchError> {
        let payload = self
            .loader
            .get(uri, None, false)
            .map_err(SearchError::Load)?;
        // Account for the XML scan and bound total metadata retained across repositories.
        self.charge(payload.text.len().div_ceil(64))?;
        Ok(payload)
    }

    fn add_links(
        &mut self,
        repo: &Repository,
        visits: &mut Visits,
        children: bool,
    ) -> Result<(), SearchError> {
        if !self.sites.contains_key(&repo.base) {
            let uri = repo
                .base
                .join("ilisite.xml")
                .expect("valid repository base");
            self.charge(1)?;
            let site = match self.payload(&uri) {
                Ok(payload) => match read_site(&payload.text, &repo.base) {
                    Ok(site) => site,
                    Err(()) => {
                        self.warn(format!("Invalid site metadata at {}", safe_uri(&uri)))?;
                        Site::default()
                    }
                },
                Err(SearchError::Load(error)) => {
                    if error != FetchError::NotFound {
                        self.warn(format!(
                            "Site metadata at {} is unavailable ({error})",
                            safe_uri(&uri)
                        ))?;
                    }
                    Site::default()
                }
                Err(error) => return Err(error),
            };
            self.sites.insert(repo.base.clone(), site);
        }
        let site = &self.sites[&repo.base];
        let count = site.parents.len() + if children { site.children.len() } else { 0 };
        self.work
            .charge(u32::try_from(count).map_err(|_| SearchError::Limit)?)
            .map_err(|_| SearchError::Limit)?;
        visits.parents.extend(site.parents.iter().cloned());
        if children {
            visits.children.extend(site.children.iter().cloned());
        }
        Ok(())
    }

    fn visit(
        &mut self,
        repo: &Repository,
        query: Query<'_>,
        visits: &mut Visits,
    ) -> Result<Option<ModelEntry>, SearchError> {
        if !self.indexes.contains_key(&repo.index) {
            self.charge(1)?;
            let index = self.payload(&repo.index).and_then(|payload| {
                read_index(&payload.text, &repo.base).map_err(|_| SearchError::Index)
            });
            if matches!(index, Err(SearchError::Limit)) {
                return Err(SearchError::Limit);
            }
            if let Err(error) = index {
                let reason = match error {
                    SearchError::Load(cause) => cause.to_string(),
                    _ => "invalid index".into(),
                };
                self.warn(format!(
                    "Repository index {} is unavailable ({reason})",
                    safe_uri(&repo.index)
                ))?;
            }
            self.indexes.insert(repo.index.clone(), index);
        }
        let entries = match &self.indexes[&repo.index] {
            Err(SearchError::Load(FetchError::NotFound)) => return Ok(None),
            Err(reason) => {
                if *reason == SearchError::Index || visits.failure.is_none() {
                    visits.failure = Some(*reason);
                }
                return Ok(None);
            }
            Ok(index) => index.get(query.name).map_or(&[][..], Vec::as_slice),
        };
        self.work
            .charge(u32::try_from(entries.len()).map_err(|_| SearchError::Limit)?)
            .map_err(|_| SearchError::Limit)?;
        let mut warnings = Vec::new();
        let mut chosen = None;
        for edition in [LanguageVersion::V24, LanguageVersion::V23] {
            if query.edition.is_some_and(|required| required != edition) {
                continue;
            }
            let mut matching = entries
                .iter()
                .filter(|entry| entry.edition == Some(edition) && !entry.browse_only);
            chosen = if let Some(version) = query.version {
                let first = matching.find(|entry| {
                    visits.other_version |= entry.version != version;
                    entry.version == version
                });
                if let Some(first) = first
                    && matching.any(|candidate| candidate.version == version && candidate != first)
                {
                    return Err(SearchError::Ambiguous);
                }
                first
            } else {
                latest(matching, &mut warnings)
            };
            if chosen.is_some() {
                break;
            }
        }
        let result = chosen.cloned().map(|mut model| {
            let priority = self.priorities.len();
            model.priority = *self
                .priorities
                .entry(repo.index.clone())
                .or_insert(priority);
            model
        });
        for reason in warnings {
            self.warn(format!(
                "Model {} in {}: {reason}",
                query.name,
                safe_uri(&repo.index)
            ))?;
        }
        Ok(result)
    }
}

fn latest<'a>(
    entries: impl Iterator<Item = &'a ModelEntry>,
    warnings: &mut Vec<&'static str>,
) -> Option<&'a ModelEntry> {
    let mut start = None;
    let mut successors: HashMap<&str, (&ModelEntry, usize)> = HashMap::new();
    for entry in entries {
        if let Some(precursor) = entry.precursor.as_deref() {
            successors
                .entry(precursor)
                .and_modify(|(_, count)| *count += 1)
                .or_insert((entry, 0));
        } else if start.is_none() {
            start = Some(entry);
        } else {
            warnings.push("duplicate start version ignored");
        }
    }
    let Some(mut current) = start else {
        if !successors.is_empty() {
            warnings.push("version chain has no start");
        }
        return None;
    };
    while let Some((next, duplicates)) = successors.remove(current.version.as_str()) {
        warnings.extend(std::iter::repeat_n(
            "duplicate successor ignored",
            duplicates,
        ));
        current = next;
    }
    if !successors.is_empty() {
        warnings.push("disconnected versions ignored");
    }
    Some(current)
}
