use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "clack",
    version,
    about = "A local-first terminal typing test",
    long_about = "Launch, type, review, and type again. No account or network is required.\nFor private custom passages, prefer --file or --stdin: --text may enter shell history."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "Machine-readable output; export selects its encoding with --format"
    )]
    pub json: bool,
    #[arg(long,value_parser=clap::value_parser!(u32).range(1..=3600),group="source")]
    pub time: Option<u32>,
    #[arg(long,value_parser=clap::value_parser!(u32).range(1..=10000),group="source")]
    pub words: Option<u32>,
    #[arg(long, conflicts_with_all=["time","words","file","text","stdin","code","zen"])]
    pub quote: bool,
    #[arg(long, group = "source")]
    pub quote_id: Option<String>,
    #[arg(long,value_parser=["short","medium","long","extended"])]
    pub length: Option<String>,
    #[arg(long, group = "source")]
    pub file: Option<PathBuf>,
    #[arg(
        long,
        group = "source",
        help = "Literal passage (may be saved by your shell history)"
    )]
    pub text: Option<String>,
    #[arg(
        long,
        group = "source",
        help = "Read bounded UTF-8 source from stdin; keyboard input still uses the controlling terminal"
    )]
    pub stdin: bool,
    #[arg(long, conflicts_with_all=["time","words","quote","quote_id","zen"])]
    pub code: bool,
    #[arg(long, group = "source")]
    pub zen: bool,
    #[arg(long)]
    pub exact: bool,
    #[arg(long)]
    pub normalize_exact: bool,
    #[arg(long,value_parser=["confirm","auto"])]
    pub completion: Option<String>,
    #[arg(long)]
    pub language: Option<String>,
    #[arg(long,num_args=0..=1,default_missing_value="true",require_equals=true)]
    pub punctuation: Option<bool>,
    #[arg(long,num_args=0..=1,default_missing_value="true",require_equals=true)]
    pub numbers: Option<bool>,
    #[arg(long)]
    pub seed: Option<u64>,
    #[arg(long)]
    pub preset: Option<String>,
    #[arg(long)]
    pub private: bool,
    #[arg(long)]
    pub once: bool,
    #[arg(long)]
    pub no_save: bool,
    #[arg(long,value_parser=["normal","expert","master"])]
    pub difficulty: Option<String>,
    #[arg(long,value_parser=["mistakes","current","full","none"])]
    pub backspace: Option<String>,
    #[arg(long,value_parser=["off","letter","word"])]
    pub stop_on_error: Option<String>,
    #[arg(long)]
    pub minimum_wpm: Option<f64>,
    #[arg(long)]
    pub minimum_accuracy: Option<f64>,
    #[arg(long,num_args=0..=1,default_missing_value="true",require_equals=true)]
    pub blind: Option<bool>,
    #[arg(long)]
    pub theme: Option<String>,
    #[arg(long,value_parser=["auto","always","off"])]
    pub focus: Option<String>,
    #[arg(long)]
    pub width: Option<String>,
    #[arg(long)]
    pub lines: Option<u8>,
    #[arg(long)]
    pub line_spacing: Option<u8>,
    #[arg(long,value_parser=clap::value_parser!(u8).range(1..=16))]
    pub tab_stop: Option<u8>,
    #[arg(long,num_args=0..=1,default_missing_value="true",require_equals=true)]
    pub ascii_markers: Option<bool>,
    #[arg(long,value_parser=["center","top"])]
    pub alignment: Option<String>,
    #[arg(long,value_parser=["bar","block","underline"])]
    pub caret: Option<String>,
    #[arg(long,value_parser=["auto","always","never"])]
    pub color: Option<String>,
    #[arg(long,num_args=0..=1,default_missing_value="true",require_equals=true)]
    pub progress: Option<bool>,
    #[arg(long,num_args=0..=1,default_missing_value="true",require_equals=true)]
    pub live_wpm: Option<bool>,
    #[arg(long,num_args=0..=1,default_missing_value="true",require_equals=true)]
    pub live_accuracy: Option<bool>,
    #[arg(long,value_parser=["wpm","cpm"])]
    pub speed_unit: Option<String>,
    #[arg(long)]
    pub pace: Option<String>,
    #[arg(long,num_args=0..=1,default_missing_value="true",require_equals=true)]
    pub auto_indent: Option<bool>,
    #[arg(long)]
    pub enhanced_keyboard: bool,
    #[arg(
        long,
        value_name = "PATH",
        help = "Write bounded numeric performance observations after terminal restoration"
    )]
    pub benchmark_output: Option<PathBuf>,
}
#[derive(Debug, Subcommand)]
pub enum Command {
    History(HistoryArgs),
    Stats(StatsArgs),
    Export(ExportArgs),
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Languages {
        #[command(subcommand)]
        command: LanguageCommand,
    },
    Themes {
        #[command(subcommand)]
        command: ThemeCommand,
    },
    Doctor {
        /// Explicitly query terminal capabilities, with a bounded 400ms wait.
        #[arg(long)]
        probe: bool,
    },
    Completions {
        shell: clap_complete::Shell,
    },
    Man,
}
#[derive(Debug, Args, Default)]
pub struct HistoryArgs {
    #[arg(long,default_value_t=20,value_parser=clap::value_parser!(u32).range(1..=1000))]
    pub limit: u32,
    #[arg(long, default_value_t = 0,value_parser=clap::value_parser!(u64).range(0..=i64::MAX as u64))]
    pub offset: u64,
    #[arg(long, default_value = "current")]
    pub profile: String,
    #[arg(long,value_parser=["time","words","quote","custom","code","zen"])]
    pub mode: Option<String>,
    #[arg(long)]
    pub language: Option<String>,
    #[arg(long,value_parser=["complete","failed","aborted","interrupted","incomplete"])]
    pub outcome: Option<String>,
    #[arg(long,value_parser=["standard","practice","paste_attempted","assisted_code"],help="Result classification, independent of completion outcome")]
    pub classification: Option<String>,
    #[arg(long)]
    pub from: Option<String>,
    #[arg(long)]
    pub to: Option<String>,
}
#[derive(Debug, Args)]
pub struct StatsArgs {
    #[arg(long, default_value = "current")]
    pub profile: String,
    #[arg(long,value_parser=["time","words","quote","custom","code","zen"])]
    pub mode: Option<String>,
    #[arg(long)]
    pub language: Option<String>,
    #[arg(long,value_parser=["complete","failed","aborted","interrupted","incomplete"])]
    pub outcome: Option<String>,
    #[arg(long,value_parser=["standard","practice","paste_attempted","assisted_code"],help="Result classification, independent of completion outcome")]
    pub classification: Option<String>,
    #[arg(long)]
    pub from: Option<String>,
    #[arg(long)]
    pub to: Option<String>,
}
#[derive(Debug, Args)]
pub struct ExportArgs {
    #[arg(long,default_value="jsonl",value_parser=["json","jsonl","csv"])]
    pub format: String,
    #[arg(long)]
    pub include_text: bool,
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long, default_value = "all")]
    pub profile: String,
    #[arg(long,value_parser=["time","words","quote","custom","code","zen"])]
    pub mode: Option<String>,
    #[arg(long)]
    pub language: Option<String>,
    #[arg(long,value_parser=["complete","failed","aborted","interrupted","incomplete"])]
    pub outcome: Option<String>,
    #[arg(long,value_parser=["standard","practice","paste_attempted","assisted_code"],help="Result classification, independent of completion outcome")]
    pub classification: Option<String>,
    #[arg(long)]
    pub from: Option<String>,
    #[arg(long)]
    pub to: Option<String>,
}
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    Path,
    Show {
        #[arg(long)]
        resolved: bool,
    },
    Validate,
}
#[derive(Debug, Subcommand)]
pub enum LanguageCommand {
    List,
    Import { path: PathBuf },
}
#[derive(Debug, Subcommand)]
pub enum ThemeCommand {
    List,
}

impl Cli {
    /// Validate command-only boundaries before commands which intentionally do
    /// not read configuration (path discovery and generated documentation).
    pub fn validate_command_contract(&self) -> Result<(), String> {
        if self.command.is_some() && self.stdin {
            return Err("--stdin is interactive source input; commands never consume it (use --file or --text for a current profile)".into());
        }
        if self.command.is_some() && (self.once || self.benchmark_output.is_some()) {
            return Err("--once and --benchmark-output apply to interactive tests only".into());
        }
        Ok(())
    }

    pub fn apply(&self, config: &mut crate::config::Config) -> Result<(), String> {
        let mut next = config.clone();
        self.apply_inner(&mut next)?;
        *config = next;
        Ok(())
    }
    fn apply_inner(&self, config: &mut crate::config::Config) -> Result<(), String> {
        use crate::{
            engine::{Mode, Policy},
            ui::Width,
        };
        fn choice<T: serde::de::DeserializeOwned>(value: &str) -> T {
            serde_json::from_value(serde_json::Value::String(value.into()))
                .expect("Clap-validated choice")
        }
        if let Some(name) = &self.preset {
            config.preset(name)?;
        }
        if let Some(seconds) = self.time {
            config.test.mode = Mode::Time;
            config.test.seconds = seconds;
            config.test.policy = Policy::Prose;
        }
        if let Some(words) = self.words {
            config.test.mode = Mode::Words;
            config.test.words = words;
            config.test.policy = Policy::Prose;
        }
        if self.quote || self.quote_id.is_some() {
            config.test.mode = Mode::Quote;
            config.test.policy = Policy::Prose;
            config.test.quote_id = self.quote_id.clone();
        }
        if self.file.is_some() || self.text.is_some() || self.stdin {
            if config.test.mode != Mode::Code {
                config.test.mode = Mode::Custom;
            }
            config.test.file = self.file.clone();
        }
        if self.code {
            config.test.mode = Mode::Code;
            config.test.policy = Policy::Exact;
        }
        if self.zen {
            config.test.mode = Mode::Zen;
            config.test.policy = Policy::Prose;
        }
        if self.exact {
            config.test.policy = Policy::Exact;
        }
        if self.normalize_exact {
            config.test.normalize_exact = true;
        }
        if let Some(value) = &self.completion {
            config.test.completion = choice(value);
        }
        if let Some(value) = &self.length {
            if config.test.mode != Mode::Quote {
                return Err("--length requires quote mode".into());
            }
            config.test.quote_length = value.clone();
        }
        if let Some(value) = &self.language {
            config.test.language = value.clone();
        }
        if let Some(value) = self.punctuation {
            config.test.punctuation = value;
        }
        if let Some(value) = self.numbers {
            config.test.numbers = value;
        }
        if let Some(value) = &self.difficulty {
            config.rules.difficulty = choice(value);
        }
        if let Some(value) = &self.backspace {
            config.rules.backspace = choice(value);
        }
        if let Some(value) = &self.stop_on_error {
            config.rules.stop_on_error = choice(value);
        }
        if let Some(value) = self.minimum_wpm {
            config.rules.minimum_wpm = Some(value);
        }
        if let Some(value) = self.minimum_accuracy {
            config.rules.minimum_accuracy = Some(value);
        }
        if let Some(value) = self.blind {
            config.rules.blind = value;
        }
        if let Some(value) = &self.theme {
            config.appearance.theme = value.clone();
        }
        if let Some(value) = &self.focus {
            config.appearance.focus = choice(value);
        }
        if let Some(value) = &self.width {
            config.appearance.width = if value == "auto" {
                Width::Auto
            } else {
                Width::Cells(
                    value
                        .parse()
                        .map_err(|_| "--width expects 40–120 or auto")?,
                )
            };
        }
        if let Some(value) = self.lines {
            config.appearance.lines = value;
        }
        if let Some(value) = self.line_spacing {
            config.appearance.line_spacing = value;
        }
        if let Some(value) = self.tab_stop {
            config.appearance.tab_stop = value;
        }
        if let Some(value) = self.ascii_markers {
            config.appearance.ascii_markers = value;
        }
        if let Some(value) = &self.alignment {
            config.appearance.alignment = choice(value);
        }
        if let Some(value) = &self.caret {
            config.appearance.caret = choice(value);
        }
        if let Some(value) = &self.color {
            config.appearance.color = choice(value);
        }
        if let Some(value) = self.progress {
            config.status.progress = value;
        }
        if let Some(value) = self.live_wpm {
            config.status.wpm = value;
        }
        if let Some(value) = self.live_accuracy {
            config.status.accuracy = value;
        }
        if let Some(value) = &self.speed_unit {
            config.status.speed_unit = choice(value);
        }
        if let Some(value) = &self.pace {
            config.practice.pace = value.clone();
        }
        if let Some(value) = self.auto_indent {
            config.practice.auto_indent = value;
        }
        if self.private || self.no_save {
            config.privacy.save_results = false;
        }
        if self.private {
            config.privacy.private_session = true;
            config.privacy.store_custom_text = false;
            config.privacy.store_event_trace = false;
        }
        if self.enhanced_keyboard {
            config.enhanced_keyboard = true;
        }
        if self.command.is_none() && self.json && !self.once {
            return Err("interactive --json requires --once".into());
        }
        self.validate_command_contract()?;
        match &self.command {
            Some(Command::History(args)) => validate_query(
                &args.profile,
                args.language.as_deref(),
                args.from.as_deref(),
                args.to.as_deref(),
            )?,
            Some(Command::Stats(args)) => validate_query(
                &args.profile,
                args.language.as_deref(),
                args.from.as_deref(),
                args.to.as_deref(),
            )?,
            Some(Command::Export(args)) => validate_query(
                &args.profile,
                args.language.as_deref(),
                args.from.as_deref(),
                args.to.as_deref(),
            )?,
            _ => {}
        }
        if self.normalize_exact && config.test.policy != Policy::Exact {
            return Err("--normalize-exact requires exact policy".into());
        }
        config.validate()
    }
}
fn validate_query(
    profile: &str,
    language: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<(), String> {
    let valid_profile = matches!(profile, "current" | "all")
        || (profile.len() == 64 && profile.bytes().all(|b| b.is_ascii_hexdigit()));
    if !valid_profile {
        return Err("--profile expects current, all, or a 64-digit profile key".into());
    }
    if language.is_some_and(|id| {
        id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    }) {
        return Err("--language filter expects a stable source ID".into());
    }
    let from = from
        .map(|value| crate::storage::parse_utc_bound(value, false))
        .transpose()
        .map_err(|error| format!("--from: {error}"))?;
    let to = to
        .map(|value| crate::storage::parse_utc_bound(value, true))
        .transpose()
        .map_err(|error| format!("--to: {error}"))?;
    if from.zip(to).is_some_and(|(from, to)| from > to) {
        return Err("--from must not be later than --to".into());
    }
    Ok(())
}
