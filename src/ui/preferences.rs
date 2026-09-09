use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Focus {
    #[default]
    Auto,
    Always,
    Off,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    #[default]
    Center,
    Top,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Caret {
    #[default]
    Bar,
    Block,
    Underline,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeedUnit {
    #[default]
    Wpm,
    Cpm,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorPolicy {
    #[default]
    Auto,
    Always,
    Never,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Width {
    #[default]
    Auto,
    Cells(u16),
}
impl Serialize for Width {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => serializer.serialize_str("auto"),
            Self::Cells(width) => serializer.serialize_u16(*width),
        }
    }
}
impl<'de> Deserialize<'de> for Width {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Value {
            Number(u16),
            Text(String),
        }
        match Value::deserialize(deserializer)? {
            Value::Number(value) => Ok(Self::Cells(value)),
            Value::Text(value) if value == "auto" => Ok(Self::Auto),
            _ => Err(serde::de::Error::custom("expected 40–120 cells or 'auto'")),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    pub theme: String,
    pub focus: Focus,
    pub width: Width,
    pub alignment: Alignment,
    pub lines: u8,
    pub line_spacing: u8,
    pub caret: Caret,
    pub tab_stop: u8,
    pub color: ColorPolicy,
    pub error_underline: bool,
    pub typed_bold: bool,
    pub ascii_markers: bool,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "terminal".into(),
            focus: Focus::Auto,
            width: Width::Auto,
            alignment: Alignment::Center,
            lines: 3,
            line_spacing: 0,
            caret: Caret::Bar,
            tab_stop: 4,
            color: ColorPolicy::Auto,
            error_underline: true,
            typed_bold: false,
            ascii_markers: false,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Status {
    pub progress: bool,
    pub wpm: bool,
    pub accuracy: bool,
    pub speed_unit: SpeedUnit,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            progress: true,
            wpm: false,
            accuracy: false,
            speed_unit: SpeedUnit::Wpm,
        }
    }
}
