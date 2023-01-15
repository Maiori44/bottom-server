#[derive(Default)]
pub struct TerminalWidgetState {
    pub stdout: String,
    pub stdin: String,
    pub offset: usize,
    pub input_offset: usize,
}
