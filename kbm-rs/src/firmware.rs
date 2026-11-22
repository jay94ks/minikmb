//! Firmware programming for the selected channel. Device writes require an
//! explicit confirmed job; parsers/tool discovery never program a board.
pub mod backend;
pub mod image;
pub mod manager;
pub mod ms2130;
pub mod native;
pub mod process;
pub mod sources;
pub mod stm32;

#[derive(
    Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Stm32,
    Ms2130,
}
