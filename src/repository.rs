//! Native CLI model acquisition. The compiler entry points use explicit text.
mod index;
mod load;

use self::{
    index::{ModelEntry, Query, Repository, Search, SearchError, safe_uri},
    load::{FetchError, Loader, read_local, verify_md5},
};
use crate::{
    CompilationReport, CompilationStatus, Frontend, LanguageVersion, Limits, RequestError,
    SourceInput, SourceSet, Span, SyntaxStatus, analyze_frontend, compilation_report,
    diagnostic::Diagnostics, frontend, validate_roots,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    fs,
    path::PathBuf,
};
use url::Url;

pub struct LocalSource {
    pub path: PathBuf,
    pub source: SourceInput,
}

#[derive(Default)]
pub struct Options {
    pub repositories: Vec<String>,
    pub cache: Option<PathBuf>,
    pub no_cache: bool,
    pub offline: bool,
}

pub fn compile(
    files: Vec<LocalSource>,
    roots: Option<&[String]>,
    options: Options,
) -> Result<CompilationReport, RequestError> {
    let limits = Limits::default();
    validate_roots(roots, &limits)?;
    if files.is_empty() && roots.is_none() {
        return Err(RequestError::new(
            "REQUEST_SOURCES",
            "Expected source files or root models",
        ));
    }
    if options.no_cache && options.cache.is_some() {
        return Err(RequestError::new(
            "REQUEST_CACHE",
            "--cache and --no-cache are mutually exclusive",
        ));
    }
    let default_search = options.repositories.is_empty();
    let configured = if default_search {
        vec![Repository::parse("https://models.interlis.ch/").expect("valid default repository")]
    } else {
        options
            .repositories
            .iter()
            .map(|value| {
                Repository::parse(value).map_err(|()| {
                    RequestError::new("REQUEST_REPOSITORY", "Invalid repository location")
                })
            })
            .collect::<Result<_, _>>()?
    };
    let cache = if options.no_cache {
        None
    } else {
        options
            .cache
            .or_else(|| std::env::home_dir().map(|home| home.join(".interlis-cache")))
    };
    let mut resolver = Resolver {
        search: Search::new(configured, Loader::new(options.offline, cache), &limits),
        limits,
        discovery_bytes: 0,
        default_search,
        sources: Vec::new(),
        names: HashSet::new(),
        loaded: HashMap::new(),
        catalog: BTreeMap::new(),
        directories: BTreeSet::new(),
        pending: VecDeque::new(),
        selected: BTreeSet::new(),
        invalid_files: BTreeSet::new(),
        findings: Vec::new(),
        limited: false,
    };
    resolver.run(files, roots)?;
    resolver.finish()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ModelId {
    file: usize,
    header: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Origin {
    Explicit,
    Neighbour,
    Repository(usize),
}

struct Loaded {
    source: SourceInput,
    origin: Origin,
    headers: Vec<Header>,
    parsed: bool,
}

struct Header {
    name: String,
    version: Option<String>,
    edition: Option<LanguageVersion>,
    at: Option<u32>,
    dependencies: Vec<Need>,
}

#[derive(Clone)]
struct Need {
    name: String,
    version: Option<String>,
    edition: Option<LanguageVersion>,
    place: Option<Place>,
}

impl Need {
    fn query(&self) -> Query<'_> {
        Query {
            name: &self.name,
            edition: self.edition,
            version: self.version.as_deref(),
        }
    }
}

#[derive(Clone, Copy)]
struct Place {
    file: usize,
    start: u32,
    end: u32,
}

impl Place {
    fn new(file: usize, span: &Span) -> Self {
        let (_, start, end) = span.sort_key();
        Self { file, start, end }
    }
}

struct Finding {
    code: &'static str,
    message: String,
    place: Option<Place>,
    operational: bool,
    warning: bool,
}

struct Resolver {
    search: Search,
    limits: Limits,
    discovery_bytes: usize,
    default_search: bool,
    sources: Vec<Loaded>,
    names: HashSet<String>,
    loaded: HashMap<Url, usize>,
    catalog: BTreeMap<String, Vec<ModelId>>,
    directories: BTreeSet<PathBuf>,
    pending: VecDeque<Need>,
    selected: BTreeSet<ModelId>,
    invalid_files: BTreeSet<usize>,
    findings: Vec<Finding>,
    limited: bool,
}

impl Resolver {
    fn run(
        &mut self,
        inputs: Vec<LocalSource>,
        roots: Option<&[String]>,
    ) -> Result<(), RequestError> {
        for input in inputs {
            let uri = Url::from_file_path(&input.path)
                .map_err(|()| RequestError::new("REQUEST_SOURCE", "Invalid local source path"))?;
            if self.default_search
                && let Some(parent) = input.path.parent()
            {
                self.directories.insert(parent.to_owned());
            }
            let file = self.add_source(uri, input.source, Origin::Explicit);
            if roots.is_none() {
                if !self.sources[file].parsed {
                    self.invalid_files.insert(file);
                }
                for header in 0..self.sources[file].headers.len() {
                    self.select(ModelId { file, header });
                }
            }
        }
        if let Some(roots) = roots {
            self.pending.extend(roots.iter().map(|name| Need {
                name: name.clone(),
                edition: None,
                version: None,
                place: None,
            }));
        }
        let initial_models = self.selected.clone();
        let initial_needs = self.pending.clone();
        while !self.limited
            && let Some(need) = self.pending.pop_front()
        {
            if self.search.charge(1).is_err() {
                self.limited = true;
                break;
            }
            let available = self.sources.len();
            let result = self.resolve(&need);
            for message in std::mem::take(&mut self.search.warnings) {
                self.say("ILI-RES-WARNING", message, need.place, false, true);
            }
            let new_competitors = self.sources[available..]
                .iter()
                .flat_map(|source| &source.headers)
                .any(|header| self.catalog[&header.name].len() > 1);
            match result {
                Ok(_) if new_competitors => {
                    // A file can introduce further models at the same or a
                    // better priority. Reconsider existing bindings before
                    // validating the now available closure.
                    self.selected.clone_from(&initial_models);
                    self.pending.clone_from(&initial_needs);
                }
                Ok(selected) => self.select(selected),
                Err(SearchError::Limit) => self.limited = true,
                Err(error) => {
                    self.unresolved(&need, error);
                    break;
                }
            }
        }
        if self.search.loader.cache_write_failed {
            self.say(
                "ILI-RES-CACHE-WRITE",
                "Repository cache could not be written".into(),
                None,
                false,
                true,
            );
        }
        Ok(())
    }

    fn select(&mut self, id: ModelId) {
        if self.selected.insert(id) {
            self.pending.extend(
                self.sources[id.file].headers[id.header]
                    .dependencies
                    .iter()
                    .cloned(),
            );
        }
    }

    fn add_source(&mut self, uri: Url, source: SourceInput, origin: Origin) -> usize {
        let file = self.sources.len();
        self.discovery_bytes = self.discovery_bytes.saturating_add(source.text.len());
        let mut parsed = false;
        let mut headers = Vec::new();
        if file >= 4096
            || self.discovery_bytes > self.limits.max_total_source_bytes as usize
            || source.text.len() > self.limits.max_source_bytes as usize
        {
            self.limited = true;
        } else {
            let snapshot = SourceInput {
                name: source.name.clone(),
                text: source.text.clone(),
            };
            match frontend(vec![snapshot], &self.limits, true) {
                Ok(front) => {
                    self.limited |= front.status() == SyntaxStatus::LimitExceeded;
                    parsed =
                        front.status() == SyntaxStatus::Parsed && front.failed_sources.is_empty();
                    for document in &front.documents {
                        for model in &document.models {
                            let mut dependencies = model
                                .imports
                                .iter()
                                .filter(|import| import.name.value != "INTERLIS")
                                .map(|import| Need {
                                    name: import.name.value.clone(),
                                    version: None,
                                    edition: Some(document.language),
                                    place: Some(Place::new(file, &import.name.span)),
                                })
                                .collect::<Vec<_>>();
                            if let Some(translation) = &model.translation {
                                dependencies.push(Need {
                                    name: translation.original.value.clone(),
                                    version: Some(translation.version.value.clone()),
                                    edition: Some(document.language),
                                    place: Some(Place::new(file, &translation.original.span)),
                                });
                            }
                            headers.push(Header {
                                name: model.name.value.clone(),
                                version: Some(model.version.value.clone()),
                                edition: Some(document.language),
                                at: Some(model.span.sort_key().1),
                                dependencies,
                            });
                        }
                    }
                    for failure in front.failed_sources {
                        headers.extend(failure.models.into_iter().map(|(name, version)| Header {
                            name,
                            version,
                            edition: None,
                            at: None,
                            dependencies: Vec::new(),
                        }));
                    }
                }
                Err(_) => self.limited = true,
            }
        }
        for (header, entry) in headers.iter().enumerate() {
            self.catalog
                .entry(entry.name.clone())
                .or_default()
                .push(ModelId { file, header });
        }
        self.names.insert(source.name.clone());
        self.loaded.insert(uri, file);
        self.sources.push(Loaded {
            source,
            origin,
            headers,
            parsed,
        });
        file
    }

    fn fetched_source(&mut self, uri: Url, text: String, origin: Origin) -> usize {
        let mut name = safe_uri(&uri);
        if name.len() > 1024 || self.names.contains(&name) {
            name = format!("cli:///repository/{}", self.sources.len() + 1);
        }
        self.add_source(uri, SourceInput { name, text }, origin)
    }

    fn local_candidate(&mut self, need: &Need) -> Result<Option<ModelId>, SearchError> {
        let Some(candidates) = self.catalog.get(&need.name) else {
            return Ok(None);
        };
        self.search.charge(candidates.len())?;
        let mut best = None;
        let mut ambiguous = false;
        for &id in candidates {
            let loaded = &self.sources[id.file];
            let header = &loaded.headers[id.header];
            if need.version.as_ref().is_some_and(|version| {
                header
                    .version
                    .as_ref()
                    .is_some_and(|actual| version != actual)
            }) || matches!(loaded.origin, Origin::Repository(_))
                && need
                    .edition
                    .is_some_and(|edition| header.edition != Some(edition))
            {
                continue;
            }
            match best {
                Some((priority, _)) if priority < loaded.origin => {}
                Some((priority, _)) if priority == loaded.origin => ambiguous = true,
                _ => {
                    best = Some((loaded.origin, id));
                    ambiguous = false;
                }
            }
        }
        if ambiguous {
            Err(SearchError::Ambiguous)
        } else {
            Ok(best.map(|(_, id)| id))
        }
    }

    fn resolve(&mut self, need: &Need) -> Result<ModelId, SearchError> {
        if let Some(id) = self.local_candidate(need)? {
            return Ok(id);
        }
        if self.default_search && !self.directories.is_empty() {
            self.neighbours(need.place)?;
            if let Some(id) = self.local_candidate(need)? {
                return Ok(id);
            }
        }
        // Imports of INTERLIS are handled by the language edition in the core.
        if need.name == "INTERLIS" {
            return Err(SearchError::NotFound);
        }
        let selected = self.search.find(need.query())?;
        self.load_model(need, selected)
    }

    fn neighbours(&mut self, place: Option<Place>) -> Result<(), SearchError> {
        let mut paths = BTreeSet::new();
        for directory in std::mem::take(&mut self.directories) {
            for entry in fs::read_dir(directory).map_err(|e| SearchError::Load(e.into()))? {
                self.search.charge(1)?;
                let path = entry.map_err(|e| SearchError::Load(e.into()))?.path();
                if path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("ili"))
                    && path.is_file()
                {
                    paths.insert(fs::canonicalize(path).map_err(|e| SearchError::Load(e.into()))?);
                }
            }
        }
        for path in paths {
            self.search.charge(1)?;
            let uri = Url::from_file_path(path).map_err(|()| SearchError::Load(FetchError::Io))?;
            if self.loaded.contains_key(&uri) {
                continue;
            }
            let payload = match read_local(&uri) {
                Ok(payload) => payload,
                Err(error) => {
                    self.say(
                        load_code(error),
                        format!("Neighbour source {}: {error}", safe_uri(&uri)),
                        place,
                        true,
                        false,
                    );
                    return Err(SearchError::Reported);
                }
            };
            self.fetched_source(uri, payload.text, Origin::Neighbour);
            if self.limited {
                return Err(SearchError::Limit);
            }
        }
        Ok(())
    }

    fn load_model(&mut self, need: &Need, entry: ModelEntry) -> Result<ModelId, SearchError> {
        let file = match self.loaded.get(&entry.uri) {
            Some(&file) => {
                verify_md5(
                    self.sources[file].source.text.as_bytes(),
                    entry.md5.as_deref(),
                )
                .map_err(|error| self.model_load_error(need, &entry.uri, error))?;
                file
            }
            None => {
                let payload = self
                    .search
                    .loader
                    .get(&entry.uri, entry.md5.as_deref(), true)
                    .map_err(|error| self.model_load_error(need, &entry.uri, error))?;
                self.fetched_source(
                    entry.uri.clone(),
                    payload.text,
                    Origin::Repository(entry.priority),
                )
            }
        };
        if self.limited {
            return Err(SearchError::Limit);
        }
        let source = &self.sources[file];
        let mut matching = source.headers.iter().enumerate().filter(|(_, header)| {
            header.name == need.name
                && header.edition == entry.edition
                && header.version.as_ref() == Some(&entry.version)
        });
        let exact = matching.next();
        let duplicate = matching.next().is_some();
        if source.parsed
            && let Some((header, _)) = exact
            && !duplicate
        {
            return Ok(ModelId { file, header });
        }
        let named = source
            .headers
            .iter()
            .find(|header| header.name == need.name);
        let (code, reason) = if duplicate {
            ("ILI-RES-AMBIGUOUS", "multiple matching declarations".into())
        } else if let Some(actual) = named
            && exact.is_none()
            && let Some(version) = &actual.version
            && *version != entry.version
        {
            (
                "ILI-RES-VERSION-MISMATCH",
                format!("found version {version:?}"),
            )
        } else if !source.parsed {
            (
                "ILI-RES-INVALID-SOURCE",
                "source has syntax errors or an unsupported edition".into(),
            )
        } else if named.is_some() {
            (
                "ILI-RES-INVALID-SOURCE",
                "source uses a different INTERLIS edition".into(),
            )
        } else {
            (
                "ILI-RES-INVALID-SOURCE",
                "model is absent from the source".into(),
            )
        };
        self.invalid_files.insert(file);
        self.say(
            code,
            format!(
                "Selected source {} for {} {:?}: {reason}",
                safe_uri(&entry.uri),
                need.name,
                entry.version
            ),
            need.place,
            false,
            false,
        );
        Err(SearchError::Reported)
    }

    fn model_load_error(&mut self, need: &Need, uri: &Url, error: FetchError) -> SearchError {
        self.say(
            load_code(error),
            format!("Model {} at {}: {error}", need.name, safe_uri(uri)),
            need.place,
            technical(error),
            false,
        );
        SearchError::Reported
    }

    fn unresolved(&mut self, need: &Need, error: SearchError) {
        let (code, reason, operational) = match error {
            SearchError::NotFound => (
                if need.version.is_some() {
                    "ILI-TRANSLATION-ORIGINAL-NOT-FOUND"
                } else if need.place.is_some() {
                    "ILI-IMPORT-NOT-FOUND"
                } else {
                    "ILI-ROOT-NOT-FOUND"
                },
                "model was not found".into(),
                false,
            ),
            SearchError::Ambiguous => (
                "ILI-RES-AMBIGUOUS",
                "more than one matching model".into(),
                false,
            ),
            SearchError::Version => (
                "ILI-RES-VERSION-MISMATCH",
                "the requested version was not found".into(),
                false,
            ),
            SearchError::Index => (
                "ILI-RES-INDEX",
                "repository index could not be read".into(),
                true,
            ),
            SearchError::Load(error) => (load_code(error), error.to_string(), true),
            SearchError::Limit | SearchError::Reported => return,
        };
        let model = if let Some(version) = &need.version {
            format!("{} {version}", need.name)
        } else {
            need.name.clone()
        };
        self.say(
            code,
            format!("{model}: {reason}"),
            need.place,
            operational,
            false,
        );
    }

    fn say(
        &mut self,
        code: &'static str,
        message: String,
        place: Option<Place>,
        operational: bool,
        warning: bool,
    ) {
        if self.findings.len() >= self.limits.max_diagnostics as usize {
            self.limited = true;
        } else {
            self.findings.push(Finding {
                code,
                message,
                place,
                operational,
                warning,
            });
        }
    }

    fn finish(self) -> Result<CompilationReport, RequestError> {
        let mut selection = BTreeMap::<usize, BTreeSet<u32>>::new();
        let mut failed_models = BTreeSet::new();
        for id in self.selected {
            let header = &self.sources[id.file].headers[id.header];
            let positions = selection.entry(id.file).or_default();
            if let Some(at) = header.at {
                positions.insert(at);
            } else {
                failed_models.insert((id.file, header.name.clone(), header.version.clone()));
            }
        }
        let mut inputs = Vec::new();
        let mut kept = BTreeMap::new();
        for (index, loaded) in self.sources.into_iter().enumerate() {
            if loaded.origin == Origin::Explicit
                || selection.contains_key(&index)
                || self.invalid_files.contains(&index)
            {
                kept.insert(loaded.source.name.clone(), index);
                inputs.push(loaded.source);
            }
        }
        let mut front = if inputs.is_empty() {
            Frontend {
                sources: SourceSet::empty(),
                documents: Vec::new(),
                diagnostics: Diagnostics::new(&self.limits),
                failed_sources: Vec::new(),
            }
        } else {
            frontend(inputs, &self.limits, true)?
        };
        let original_ids = front
            .sources
            .files()
            .iter()
            .map(|file| kept[file.name()])
            .collect::<Vec<_>>();
        let actual_ids = original_ids
            .iter()
            .enumerate()
            .map(|(index, &original)| (original, index))
            .collect::<HashMap<_, _>>();
        for document in &mut front.documents {
            let file = original_ids[document.span.sort_key().0 as usize];
            document.models.retain(|model| {
                selection
                    .get(&file)
                    .is_some_and(|offsets| offsets.contains(&model.span.sort_key().1))
            });
        }
        for failure in front.failed_sources.drain(..) {
            let file = original_ids[failure.source as usize];
            if self.invalid_files.contains(&file)
                || failure.models.iter().any(|(name, version)| {
                    failed_models.contains(&(file, name.clone(), version.clone()))
                })
            {
                front.diagnostics.absorb(failure.diagnostics);
            }
        }
        let mut operational = false;
        for finding in self.findings {
            let span = finding.place.and_then(|place| {
                let &index = actual_ids.get(&place.file)?;
                front
                    .sources
                    .span(index, place.start as usize, place.end as usize)
            });
            operational |= finding.operational;
            if finding.warning {
                front
                    .diagnostics
                    .warning(finding.code, &finding.message, span);
            } else {
                front.diagnostics.push(finding.code, &finding.message, span);
            }
        }
        if self.limited {
            front
                .diagnostics
                .limit("Model acquisition budget exceeded", None);
        }
        let mut report = compilation_report(analyze_frontend(front, None, &self.limits));
        if operational && report.status != CompilationStatus::LimitExceeded {
            report.status = CompilationStatus::InternalError;
        }
        Ok(report)
    }
}

fn technical(error: FetchError) -> bool {
    !matches!(error, FetchError::InvalidSource | FetchError::Checksum)
}

fn load_code(error: FetchError) -> &'static str {
    match error {
        FetchError::Offline => "ILI-RES-OFFLINE",
        FetchError::InvalidSource => "ILI-RES-INVALID-SOURCE",
        FetchError::Checksum => "ILI-RES-CHECKSUM",
        FetchError::Timeout => "ILI-RES-TIMEOUT",
        _ => "ILI-RES-LOAD",
    }
}

pub fn check_json(report: &CompilationReport) -> Result<String, RequestError> {
    crate::wire::encode_check_report(report)
}
