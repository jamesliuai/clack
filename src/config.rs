//! Typed session settings. CLI overrides do not write this configuration.
use crate::{
    engine::{Completion, GeneratorParameters, Mode, Policy, Rules, TestSpec},
    settings::{self, Bindings, Edit, Kind},
    ui::{Appearance, Status},
};
use serde::{Deserialize, Serialize};
#[cfg(not(windows))]
use std::fs::OpenOptions;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TestSettings {
    pub mode: Mode,
    pub seconds: u32,
    pub words: u32,
    pub language: String,
    pub punctuation: bool,
    pub numbers: bool,
    pub policy: Policy,
    pub normalize_exact: bool,
    pub completion: Completion,
    pub quote_length: String,
    pub quote_id: Option<String>,
    pub file: Option<PathBuf>,
    pub generator_parameters: GeneratorParameters,
}
impl Default for TestSettings {
    fn default() -> Self {
        Self {
            mode: Mode::Time,
            seconds: 30,
            words: 50,
            language: "english_200".into(),
            punctuation: false,
            numbers: false,
            policy: Policy::Prose,
            normalize_exact: false,
            completion: Completion::Confirm,
            quote_length: "short".into(),
            quote_id: None,
            file: None,
            generator_parameters: GeneratorParameters::default(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Practice {
    pub pace: String,
    pub auto_indent: bool,
    pub selection: String,
}
impl Default for Practice {
    fn default() -> Self {
        Self {
            pace: "off".into(),
            auto_indent: false,
            selection: "missed".into(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Privacy {
    pub save_results: bool,
    pub store_custom_text: bool,
    pub store_event_trace: bool,
    pub private_session: bool,
}
impl Default for Privacy {
    fn default() -> Self {
        Self {
            save_results: true,
            store_custom_text: false,
            store_event_trace: false,
            private_session: false,
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Workflow {
    pub favorite_themes: Vec<String>,
    pub favorite_packs: Vec<String>,
    pub result_details: bool,
    pub bindings: BTreeMap<String, String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Storage {
    pub journal_mode: String,
    pub pending_limit: usize,
}
impl Default for Storage {
    fn default() -> Self {
        Self {
            journal_mode: "auto".into(),
            pending_limit: 16,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub test: TestSettings,
    pub appearance: Appearance,
    pub status: Status,
    pub rules: Rules,
    pub practice: Practice,
    pub privacy: Privacy,
    pub workflow: Workflow,
    pub storage: Storage,
    pub presets: BTreeMap<String, toml::Table>,
    pub enhanced_keyboard: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            test: TestSettings::default(),
            appearance: Appearance::default(),
            status: Status::default(),
            rules: Rules::default(),
            practice: Practice::default(),
            privacy: Privacy::default(),
            workflow: Workflow::default(),
            storage: Storage::default(),
            presets: BTreeMap::new(),
            enhanced_keyboard: false,
        }
    }
}
impl Config {
    pub fn test_spec(&self) -> TestSpec {
        TestSpec {
            mode: self.test.mode,
            seconds: self.test.seconds,
            words: self.test.words,
            source_id: self.test.language.clone(),
            punctuation: self.test.punctuation,
            numbers: self.test.numbers,
            generator_parameters: self.test.generator_parameters,
            policy: self.test.policy,
            normalize: self.test.policy == Policy::Prose || self.test.normalize_exact,
            completion: self.test.completion,
            rules: self.rules.clone(),
            pace_wpm: self.practice.pace.parse().ok(),
            auto_indent: self.practice.auto_indent,
            ..TestSpec::default()
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        let value = toml::Value::try_from(self).map_err(|error| error.to_string())?;
        for setting in settings::REGISTRY {
            setting.validate(settings::value_at(&value, setting.path))?;
        }
        self.test_spec().validate()?;
        validate_pack_id(&self.test.language, "test.language")?;
        for pack in &self.workflow.favorite_packs {
            validate_pack_id(pack, "workflow.favorite_packs")?;
        }
        for theme in &self.workflow.favorite_themes {
            if !crate::ui::THEMES.contains(&theme.as_str()) {
                return Err(format!(
                    "workflow.favorite_themes={theme:?}: unknown shipped theme"
                ));
            }
        }
        Bindings::from_config(self)?;
        for (name, table) in &self.presets {
            validate_preset_name(name)?;
            validate_fields(&toml::Value::Table(table.clone()), "", false)
                .map_err(|error| format!("presets.{name}.{error}"))?;
        }
        Ok(())
    }
    pub fn read(path: &Path) -> Result<Self, String> {
        Self::from_text(
            &String::from_utf8(read_bytes(path)?)
                .map_err(|_| "configuration must be valid UTF-8".to_owned())?,
        )
    }
    pub fn from_text(text: &str) -> Result<Self, String> {
        if text.len() > MAX_CONFIG_BYTES {
            return Err("configuration exceeds256 KiB".into());
        }
        let value: toml::Value = toml::from_str(text)
            .map_err(|error| format!("invalid configuration syntax: {}", error.message()))?;
        Self::from_value(value)
    }
    fn from_value(value: toml::Value) -> Result<Self, String> {
        validate_fields(&value, "", true)?;
        let config: Self = value.try_into().map_err(|error: toml::de::Error| {
            format!("invalid configuration: {}", error.message())
        })?;
        config.validate()?;
        Ok(config)
    }
    /// Apply edits transactionally to an effective session, without file I/O.
    pub fn edited(&self, edits: &[Edit]) -> Result<Self, String> {
        let mut value = toml::Value::try_from(self).map_err(|error| error.to_string())?;
        for edit in edits {
            let setting = settings::find(&edit.path)
                .ok_or_else(|| format!("unknown setting {:?}", edit.path))?;
            setting.validate(edit.value.as_ref())?;
            set_value(
                &mut value,
                &edit.path.split('.').collect::<Vec<_>>(),
                edit.value.clone(),
            )?;
        }
        Self::from_value(value)
    }
    /// Persist only edited fields. Unrelated effective CLI overrides never enter
    /// the file; failure leaves both the session and existing file unchanged.
    pub fn persist_edits(&mut self, path: &Path, edits: &[Edit]) -> Result<(), String> {
        let next = self.edited(edits)?;
        let (original, mut document, disk) = read_document(path)?;
        disk.edited(edits)?;
        for edit in edits {
            set_document(
                document.as_table_mut(),
                &edit.path.split('.').collect::<Vec<_>>(),
                edit.value.as_ref(),
            )?;
        }
        commit_document(path, original.as_deref(), &document)?;
        *self = next;
        Ok(())
    }
    pub fn save_defaults(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let (original, mut document, _) = read_document(path)?;
        for setting in settings::REGISTRY {
            set_document(
                document.as_table_mut(),
                &setting.path.split('.').collect::<Vec<_>>(),
                setting.value(self).as_ref(),
            )?;
        }
        commit_document(path, original.as_deref(), &document)
    }
    pub fn preset_names(&self) -> Vec<String> {
        ["default", "focused", "code"]
            .into_iter()
            .map(str::to_owned)
            .chain(self.presets.keys().cloned())
            .collect()
    }
    pub fn preset(&mut self, name: &str) -> Result<(), String> {
        let mut next = self.clone();
        match name {
            "default" => {
                next = Self::default();
                next.presets = self.presets.clone();
            }
            "focused" => {
                next.appearance.focus = crate::ui::Focus::Always;
                next.status.progress = false;
                next.status.wpm = false;
                next.status.accuracy = false;
            }
            "code" => {
                next.test.mode = Mode::Code;
                next.test.policy = Policy::Exact;
                next.test.completion = Completion::Confirm;
            }
            _ => {
                let Some(preset) = self.presets.get(name) else {
                    return Err(format!("unknown preset {name:?}"));
                };
                let mut base = toml::Value::try_from(&*self).map_err(|error| error.to_string())?;
                merge(&mut base, &toml::Value::Table(preset.clone()));
                next = Self::from_value(base)?;
            }
        }
        next.validate()?;
        *self = next;
        Ok(())
    }
    pub fn persist_preset(&mut self, path: &Path, name: &str) -> Result<(), String> {
        let edits = self.preset_edits(name)?;
        self.persist_edits(path, &edits)
    }
    pub fn preset_edits(&self, name: &str) -> Result<Vec<Edit>, String> {
        let mut next = self.clone();
        next.preset(name)?;
        let paths: Vec<&str> = match name {
            "default" => settings::REGISTRY
                .iter()
                .filter(|setting| setting.path != "presets")
                .map(|setting| setting.path)
                .collect(),
            "focused" => vec![
                "appearance.focus",
                "status.progress",
                "status.wpm",
                "status.accuracy",
            ],
            "code" => vec!["test.mode", "test.policy", "test.completion"],
            _ => {
                let data = toml::Value::Table(self.presets[name].clone());
                settings::REGISTRY
                    .iter()
                    .filter(|setting| settings::value_at(&data, setting.path).is_some())
                    .map(|setting| setting.path)
                    .collect()
            }
        };
        Ok(paths
            .into_iter()
            .map(|path| Edit {
                path: path.into(),
                value: settings::find(path).expect("registry path").value(&next),
            })
            .collect())
    }
    pub fn save_preset(&mut self, path: &Path, name: &str) -> Result<(), String> {
        validate_preset_name(name)?;
        self.validate()?;
        let mut data = toml::Value::try_from(&*self).map_err(|error| error.to_string())?;
        let table = data.as_table_mut().expect("Config serializes as table");
        table.remove("presets");
        table.remove("schema_version");
        let mut next = self.clone();
        next.presets.insert(name.into(), table.clone());
        next.validate()?;
        let (original, mut document, mut disk) = read_document(path)?;
        disk.presets.insert(name.into(), table.clone());
        disk.validate()?;
        set_document(document.as_table_mut(), &["presets", name], Some(&data))?;
        commit_document(path, original.as_deref(), &document)?;
        *self = next;
        Ok(())
    }
}
const MAX_CONFIG_BYTES: usize = 262_144;
fn validate_pack_id(id: &str, path: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 64
        || id.starts_with('.')
        || id.contains("..")
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
    {
        return Err(format!("{path}={id:?}: expected a stable ASCII pack ID"));
    }
    Ok(())
}
fn validate_preset_name(name: &str) -> Result<(), String> {
    if matches!(name, "default" | "focused" | "code") {
        return Err(format!(
            "presets.{name}: built-in preset names are reserved"
        ));
    }
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
    {
        return Err(format!(
            "presets.{name:?}: use1–64 ASCII letters, digits, underscore or hyphen"
        ));
    }
    Ok(())
}
fn validate_fields(value: &toml::Value, prefix: &str, allow_presets: bool) -> Result<(), String> {
    let table = value
        .as_table()
        .ok_or_else(|| format!("{prefix}: expected a settings table"))?;
    for (key, value) in table {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        if !settings::safe_text(&path) {
            return Err(format!("invalid setting name {path:?}"));
        }
        if let Some(setting) = settings::find(&path) {
            if matches!(setting.kind, Kind::Presets) && !allow_presets {
                return Err(format!("{path}: nested presets are not allowed"));
            }
            setting.validate(Some(value))?;
        } else if settings::REGISTRY
            .iter()
            .any(|setting| setting.path.starts_with(&(path.clone() + ".")))
        {
            validate_fields(value, &path, allow_presets)?;
        } else {
            return Err(format!("{path}={value:?}: unknown setting"));
        }
    }
    Ok(())
}
fn set_value(
    root: &mut toml::Value,
    path: &[&str],
    value: Option<toml::Value>,
) -> Result<(), String> {
    let table = root
        .as_table_mut()
        .ok_or_else(|| format!("{}: expected table", path.join(".")))?;
    if path.len() == 1 {
        if let Some(value) = value {
            table.insert(path[0].into(), value);
        } else {
            table.remove(path[0]);
        }
    } else {
        let entry = table
            .entry(path[0].to_owned())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        set_value(entry, &path[1..], value)?;
    }
    Ok(())
}
fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| format!("cannot read configuration: {error}"))?
        .take(MAX_CONFIG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read configuration: {error}"))?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err("configuration exceeds256 KiB".into());
    }
    Ok(bytes)
}
fn read_document(path: &Path) -> Result<(Option<Vec<u8>>, toml_edit::DocumentMut, Config), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err("configuration is a symbolic link; edit its target explicitly".into());
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err("configuration path is not a regular file".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((None, toml_edit::DocumentMut::new(), Config::default()));
        }
        Err(error) => return Err(format!("cannot inspect configuration: {error}")),
    }
    let bytes = read_bytes(path)?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| "configuration must be valid UTF-8".to_owned())?;
    let config = Config::from_text(text)?;
    let document = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| format!("invalid configuration syntax: {error}"))?;
    Ok((Some(bytes), document, config))
}
fn set_document(
    table: &mut dyn toml_edit::TableLike,
    path: &[&str],
    value: Option<&toml::Value>,
) -> Result<(), String> {
    if path.len() > 1 {
        if !table.contains_key(path[0]) {
            let mut child = toml_edit::Table::new();
            child.set_implicit(true);
            table.insert(path[0], toml_edit::Item::Table(child));
        }
        let child = table
            .get_mut(path[0])
            .and_then(toml_edit::Item::as_table_like_mut)
            .ok_or_else(|| format!("{}: expected a settings table", path[0]))?;
        return set_document(child, &path[1..], value);
    }
    let key = path[0];
    let Some(value) = value else {
        table.remove(key);
        return Ok(());
    };
    if let Some(values) = value.as_table() {
        if !table.contains_key(key) {
            table.insert(key, toml_edit::Item::Table(toml_edit::Table::new()));
        }
        let child = table
            .get_mut(key)
            .and_then(toml_edit::Item::as_table_like_mut)
            .ok_or_else(|| format!("{key}: expected a settings table"))?;
        let old: Vec<String> = child.iter().map(|(key, _)| key.into()).collect();
        for old in old {
            if !values.contains_key(&old) {
                child.remove(&old);
            }
        }
        for (key, value) in values {
            set_document(child, &[key], Some(value))?;
        }
    } else {
        let encoded = format!("value = {value}");
        let mut replacement = encoded
            .parse::<toml_edit::DocumentMut>()
            .map_err(|error| error.to_string())?["value"]
            .clone();
        if let (Some(old), Some(new)) = (
            table.get(key).and_then(toml_edit::Item::as_value),
            replacement.as_value_mut(),
        ) {
            *new.decor_mut() = old.decor().clone();
        }
        table.insert(key, replacement);
    }
    Ok(())
}
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
fn commit_document(
    path: &Path,
    original: Option<&[u8]>,
    document: &toml_edit::DocumentMut,
) -> Result<(), String> {
    let text = document.to_string();
    Config::from_text(&text)?;
    #[cfg(windows)]
    clack_private_fs::validate_file_path(path)
        .map_err(|error| format!("invalid private configuration destination: {error}"))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    #[cfg(windows)]
    let created = clack_private_fs::create_dir_all(parent);
    #[cfg(not(windows))]
    let created = {
        let mut directories = fs::DirBuilder::new();
        directories.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            directories.mode(0o700);
        }
        directories.create(parent)
    };
    created.map_err(|error| format!("cannot create configuration directory: {error}"))?;
    let (temporary, mut file) = (0..100)
        .find_map(|_| {
            let temporary = parent.join(format!(
                ".clack-config-{}-{}.tmp",
                std::process::id(),
                TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            #[cfg(windows)]
            let created = clack_private_fs::create_new_file(&temporary);
            #[cfg(not(windows))]
            let created = {
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                options.open(&temporary)
            };
            match created {
                Ok(file) => Some(Ok((temporary, file))),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(error) => Some(Err(format!(
                    "cannot create temporary configuration: {error}"
                ))),
            }
        })
        .ok_or_else(|| "cannot allocate a unique temporary configuration".to_owned())??;
    let result = (|| {
        file.write_all(text.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("cannot write configuration: {error}"))?;
        drop(file);
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("configuration changed to a symbolic link during edit".into());
            }
            Ok(_) => {
                if original != Some(read_bytes(path)?.as_slice()) {
                    return Err("configuration changed concurrently; retry the edit".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && original.is_none() => {}
            Err(error) => return Err(format!("configuration changed during edit: {error}")),
        }
        fs::rename(&temporary, path)
            .map_err(|error| format!("cannot atomically replace configuration: {error}"))?;
        // Rename is the commit point. Directory sync is best effort because some
        // supported filesystems do not expose it; never report a post-commit edit
        // failure while leaving the caller's effective session stale.
        #[cfg(unix)]
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
fn merge(base: &mut toml::Value, patch: &toml::Value) {
    if let (Some(base), Some(patch)) = (base.as_table_mut(), patch.as_table()) {
        for (key, value) in patch {
            if let Some(existing) = base.get_mut(key) {
                merge(existing, value);
            } else {
                base.insert(key.clone(), value.clone());
            }
        }
    } else {
        *base = patch.clone();
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Paths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub database: PathBuf,
    pub languages: PathBuf,
}
impl Paths {
    pub fn resolve(config: Option<&Path>, data: Option<&Path>) -> Result<Self, String> {
        let defaults = directories::ProjectDirs::from("dev", "clack", "clack").ok_or(
            "platform configuration/data directories unavailable; provide --config and --data-dir",
        )?;
        let data = data.unwrap_or(defaults.data_dir()).to_owned();
        Ok(Self {
            config: config
                .map_or_else(|| defaults.config_dir().join("config.toml"), Path::to_owned),
            database: data.join("history.sqlite3"),
            languages: data.join("languages"),
            data,
        })
    }
}
