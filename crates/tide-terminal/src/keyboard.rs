//! Bounded keyboard negotiation for modes the host can actually forward.
#[derive(Default)]
pub(crate) struct Layer {
    pub flags: u8,
    pub stack: Vec<u8>,
}
#[derive(Default)]
pub(crate) struct KeyboardState {
    pub layers: [Layer; 2],
    pub modify_other_keys: u8,
}
