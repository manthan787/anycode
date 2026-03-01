pub mod agents;
pub mod build;
pub mod done;
pub mod messaging;
pub mod prerequisites;
pub mod review;
pub mod sandbox;
pub mod welcome;

use crossterm::event::KeyEvent;
use ratatui::Frame;

use crate::data::WizardData;

/// What a step wants the app to do after handling an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepAction {
    Nothing,
    NextStep,
    PrevStep,
    Quit,
}

/// Trait implemented by each wizard step.
pub trait Step {
    /// Handle a key event. Returns an action for the app to perform.
    fn handle_key(&mut self, key: KeyEvent, data: &mut WizardData) -> StepAction;

    /// Render the step content into the given area.
    fn render(&self, frame: &mut Frame, area: ratatui::layout::Rect, data: &WizardData);

    /// Called when the step becomes the active step (entering from either direction).
    fn on_enter(&mut self, _data: &WizardData) {}

    /// Called every tick of the main loop (for steps that run subprocesses).
    fn tick(&mut self, _data: &WizardData) {}
}
