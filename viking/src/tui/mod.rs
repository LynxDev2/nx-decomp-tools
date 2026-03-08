use ratatui::{crossterm::event::KeyEvent, Frame};

pub trait TuiWindow {
    fn draw(&self, frame: &mut Frame);
    /// Returns true if the application should quit as a result of a key event
    fn handle_key(&mut self, key: KeyEvent) -> bool;
}

pub mod file_list_editor;
pub mod file_list_viewer;
