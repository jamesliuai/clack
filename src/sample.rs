//! Source preparation and deterministic lookahead, never called from Engine::apply.
use crate::{
    cli::Cli,
    config::{Config, Paths},
    content::{
        self, Generator, InputPolicy, LanguagePack, Modifiers, PreparedText, QuoteLength, Quotes,
        SplitMix64,
    },
    engine::{Engine, Mode, Policy},
};
use std::{
    fs::File,
    io::{self, Read},
    path::PathBuf,
    sync::Arc,
};

#[derive(Clone)]
pub struct Samples {
    pack: Option<Arc<LanguagePack>>,
    custom: Option<PreparedText>,
    random: SplitMix64,
    first_seed: Option<u64>,
    explicit_seed: bool,
    last_quote: Option<String>,
    original: Option<Vec<u8>>,
    source_file: Option<PathBuf>,
}
pub struct Sample {
    pub engine: Engine,
    pub text: String,
    pub generator: Option<Generator>,
}
impl Samples {
    pub fn prepare(cli: &Cli, config: &Config, paths: &Paths) -> Result<Self, String> {
        let random = matches!(config.test.mode, Mode::Time | Mode::Words);
        let pack = if random {
            Some(load_pack(&config.test.language, paths)?)
        } else {
            None
        };
        let policy = if config.test.policy == Policy::Exact {
            InputPolicy::Exact
        } else {
            InputPolicy::Prose
        };
        let bytes = if let Some(text) = &cli.text {
            Some(text.as_bytes().to_vec())
        } else if cli.stdin {
            Some(
                read_bounded(io::stdin().lock(), content::MAX_CUSTOM_BYTES)
                    .map_err(|error| format!("cannot read source input: {error}"))?,
            )
        } else if let Some(file) = cli.file.as_ref().or(config.test.file.as_ref()) {
            if matches!(config.test.mode, Mode::Custom | Mode::Code) {
                let file = File::open(file).map_err(|error| {
                    format!("cannot open custom source ({}): {error}", error.kind())
                })?;
                Some(
                    read_bounded(file, content::MAX_CUSTOM_BYTES)
                        .map_err(|error| format!("cannot read custom source: {error}"))?,
                )
            } else {
                None
            }
        } else {
            None
        };
        let custom = if let Some(bytes) = &bytes {
            Some(
                content::prepare_custom(bytes, policy, config.test.normalize_exact)
                    .map_err(|error| error.to_string())?,
            )
        } else {
            None
        };
        if config.test.mode == Mode::Custom && custom.is_none() {
            return Err("custom mode needs --file, --text, --stdin, or test.file".into());
        }
        if config.test.mode == Mode::Quote {
            let quotes = Quotes::bundled().map_err(|error| error.to_string())?;
            if let Some(id) = &config.test.quote_id {
                quotes.get(id).map_err(|error| error.to_string())?;
            }
        }
        let seed = cli.seed.unwrap_or_else(fresh_seed);
        Ok(Self {
            pack,
            custom,
            random: SplitMix64::new(seed),
            first_seed: Some(seed),
            explicit_seed: cli.seed.is_some(),
            last_quote: None,
            original: bytes,
            source_file: config.test.file.clone(),
        })
    }
    pub fn next(&mut self, config: &Config) -> Result<Sample, String> {
        let seed = self
            .first_seed
            .take()
            .unwrap_or_else(|| self.random.next_u64());
        let mut spec = config.test_spec();
        spec.seed = seed;
        spec.explicit_seed = self.explicit_seed;
        let mut generator = None;
        let text = match config.test.mode {
            Mode::Time | Mode::Words => {
                let pack = self
                    .pack
                    .as_ref()
                    .ok_or("selected language is not prepared")?;
                spec.source_id = pack.metadata.id.clone();
                spec.source_revision = pack.metadata.revision.clone();
                spec.content_hash = pack.metadata.content_hash.clone();
                spec.approved_content =
                    content::BUNDLED_PACK_IDS.contains(&pack.metadata.id.as_str());
                spec.generator_version = content::GENERATOR_VERSION;
                let parameters = spec.generator_parameters;
                let modifiers = Modifiers {
                    punctuation: spec.punctuation,
                    numbers: spec.numbers,
                    sentence_min_words: parameters.sentence_min_words,
                    sentence_max_words: parameters.sentence_max_words,
                    comma_percent: parameters.comma_percent,
                    number_percent: parameters.number_percent,
                    number_min_digits: parameters.number_min_digits,
                    number_max_digits: parameters.number_max_digits,
                };
                let mut stream = Generator::from_shared(Arc::clone(pack), seed, modifiers)
                    .map_err(|error| error.to_string())?;
                if config.test.mode == Mode::Words {
                    stream
                        .finite_words(config.test.words as usize)
                        .map_err(|error| error.to_string())?
                        .text
                } else {
                    let text = stream.next_chunk().map_err(|error| error.to_string())?.text;
                    generator = Some(stream);
                    text
                }
            }
            Mode::Quote => {
                let quotes = Quotes::bundled().map_err(|error| error.to_string())?;
                let quote = if let Some(id) = &config.test.quote_id {
                    quotes.get(id).map_err(|error| error.to_string())?
                } else {
                    quotes
                        .select(
                            seed,
                            quote_length(&config.test.quote_length),
                            self.last_quote.as_deref(),
                        )
                        .map_err(|error| error.to_string())?
                };
                self.last_quote = Some(quote.id.clone());
                spec.source_id = quote.id.clone();
                spec.source_revision = quote.revision.clone();
                spec.approved_content = true;
                let prepared =
                    content::prepare_custom(quote.text.as_bytes(), InputPolicy::Prose, false)
                        .map_err(|error| error.to_string())?;
                spec.content_hash = prepared.content_hash;
                prepared.text
            }
            Mode::Custom | Mode::Code => {
                let prepared = if let Some(custom) = &self.custom {
                    custom.clone()
                } else {
                    content::code_preset().map_err(|error| error.to_string())?
                };
                spec.source_id = if config.test.mode == Mode::Code {
                    "code".into()
                } else {
                    "custom".into()
                };
                spec.source_revision = "1".into();
                spec.content_hash = prepared.content_hash;
                spec.approved_content = self.custom.is_none();
                prepared.text
            }
            Mode::Zen => {
                spec.source_id = "zen".into();
                spec.source_revision = "1".into();
                spec.approved_content = false;
                String::new()
            }
        };
        let engine = Engine::new(spec, &text)?;
        let mut sample = Sample {
            engine,
            text,
            generator,
        };
        sample.lookahead(1200)?;
        Ok(sample)
    }
    pub fn repeat(&mut self, sample: &Sample) -> Result<Sample, String> {
        let mut spec = sample.engine.spec().clone();
        spec.repeated = true;
        spec.practice_reason = Some("repeated sample".into());
        Ok(Sample {
            engine: Engine::new(spec, &sample.text)?,
            text: sample.text.clone(),
            generator: sample.generator.clone(),
        })
    }
    pub fn practice(
        &mut self,
        config: &Config,
        from: &Sample,
        kind: content::PracticeKind,
    ) -> Result<Sample, String> {
        let candidates = match kind {
            content::PracticeKind::Missed => from.engine.missed_words(),
            content::PracticeKind::Slow => from.engine.slow_words(),
        };
        if candidates.is_empty() {
            return Err(match kind {content::PracticeKind::Missed=>"No mistaken or omitted words to practice",content::PracticeKind::Slow=>"Slow practice needs eight eligible correctly completed words without corrections"}.into());
        }
        let seed = self.random.next_u64();
        let prepared =
            content::prepare_practice(&candidates, seed).map_err(|error| error.to_string())?;
        let mut spec = config.test_spec();
        spec.seed = seed;
        spec.explicit_seed = false;
        spec.source_id = match kind {
            content::PracticeKind::Missed => "practice_missed",
            content::PracticeKind::Slow => "practice_slow",
        }
        .into();
        spec.source_revision = "1".into();
        spec.content_hash = prepared.content_hash;
        spec.approved_content = from.engine.spec().approved_content;
        spec.practice_reason = Some(
            match kind {
                content::PracticeKind::Missed => "missed-word practice",
                content::PracticeKind::Slow => "slow-word practice",
            }
            .into(),
        );
        Ok(Sample {
            engine: Engine::new(spec, &prepared.text)?,
            text: prepared.text,
            generator: None,
        })
    }
    pub fn reconfigure(&mut self, config: &Config, paths: &Paths) -> Result<(), String> {
        let pack = if matches!(config.test.mode, Mode::Time | Mode::Words) {
            Some(load_pack(&config.test.language, paths)?)
        } else {
            self.pack.clone()
        };
        let original = if config.test.file != self.source_file {
            config
                .test
                .file
                .as_ref()
                .map(|path| {
                    let file = File::open(path)
                        .map_err(|error| format!("cannot open custom source: {}", error.kind()))?;
                    read_bounded(file, content::MAX_CUSTOM_BYTES)
                        .map_err(|error| format!("cannot read custom source: {error}"))
                })
                .transpose()?
        } else {
            self.original.clone()
        };
        let policy = if config.test.policy == Policy::Exact {
            InputPolicy::Exact
        } else {
            InputPolicy::Prose
        };
        let custom = original
            .as_ref()
            .map(|bytes| {
                content::prepare_custom(bytes, policy, config.test.normalize_exact)
                    .map_err(|error| error.to_string())
            })
            .transpose()?;
        if config.test.mode == Mode::Custom && custom.is_none() {
            return Err("custom mode needs a source file or initial --text/--stdin content".into());
        }
        if config.test.mode == Mode::Quote {
            let quotes = Quotes::bundled().map_err(|error| error.to_string())?;
            if let Some(id) = &config.test.quote_id {
                quotes.get(id).map_err(|error| error.to_string())?;
            }
        }
        self.pack = pack;
        self.original = original;
        self.custom = custom;
        self.source_file = config.test.file.clone();
        Ok(())
    }
}
impl Sample {
    pub fn lookahead(&mut self, minimum_units: usize) -> Result<(), String> {
        while let Some(generator) = &mut self.generator {
            if self
                .engine
                .prepared_target_units()
                .saturating_sub(self.engine.logical_target_position())
                >= minimum_units
            {
                break;
            }
            let next = generator.next_chunk().map_err(|error| error.to_string())?;
            self.engine.append_prepared(&next.text)?;
            self.text.push(' ');
            self.text.push_str(&next.text);
        }
        Ok(())
    }
}
pub fn load_pack(id: &str, paths: &Paths) -> Result<Arc<LanguagePack>, String> {
    if content::BUNDLED_PACK_IDS.contains(&id) {
        return content::bundled_pack(id)
            .map(|pack| Arc::new(pack.clone()))
            .map_err(|error| error.to_string());
    }
    if id.is_empty()
        || id.len() > 64
        || id.starts_with('.')
        || id.contains("..")
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err("invalid language ID".into());
    }
    let directory = paths.languages.join(id);
    let words = read_bounded(
        File::open(directory.join("words.txt"))
            .map_err(|_| "unknown language; use 'clack languages list'")?,
        content::MAX_PACK_BYTES,
    )
    .map_err(|error| error.to_string())?;
    let metadata = read_bounded(
        File::open(directory.join("metadata.json")).map_err(|_| "language metadata is missing")?,
        64 * 1024,
    )
    .map_err(|error| error.to_string())?;
    let pack = LanguagePack::from_parts(&words, &metadata).map_err(|error| error.to_string())?;
    if pack.metadata.id != id {
        return Err("imported language directory and metadata ID disagree".into());
    }
    Ok(Arc::new(pack))
}
pub fn read_bounded(reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("source exceeds {limit} bytes"),
        ));
    }
    Ok(bytes)
}
fn quote_length(value: &str) -> QuoteLength {
    match value {
        "medium" => QuoteLength::Medium,
        "long" => QuoteLength::Long,
        "extended" => QuoteLength::Extended,
        _ => QuoteLength::Short,
    }
}
fn fresh_seed() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    (now as u64)
        ^ (now >> 64) as u64
        ^ NEXT.fetch_add(1, Ordering::Relaxed)
        ^ u64::from(std::process::id())
}
