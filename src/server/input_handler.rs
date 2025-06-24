use crate::common::protocol::{MouseButton, Modifiers};
use enigo::{Enigo, Key, Direction, Coordinate, Button, Settings, Keyboard, Mouse};
use anyhow::Result;

pub struct InputHandler {
    enigo: Enigo,
}

impl InputHandler {
    pub fn new() -> Result<Self> {
        let enigo = Enigo::new(&Settings::default())?;
        Ok(InputHandler { enigo })
    }
    
    pub fn mouse_move(&mut self, x: i32, y: i32) -> Result<()> {
        self.enigo.move_mouse(x, y, Coordinate::Abs)?;
        Ok(())
    }
    
    pub fn mouse_click(&mut self, button: MouseButton, pressed: bool, x: i32, y: i32) -> Result<()> {
        self.enigo.move_mouse(x, y, Coordinate::Abs)?;
        
        let enigo_button = match button {
            MouseButton::Left => Button::Left,
            MouseButton::Right => Button::Right,
            MouseButton::Middle => Button::Middle,
        };
        
        let direction = if pressed {
            Direction::Press
        } else {
            Direction::Release
        };
        
        self.enigo.button(enigo_button, direction)?;
        
        Ok(())
    }
    
    pub fn key_event(&mut self, key_str: &str, pressed: bool, _modifiers: Modifiers) -> Result<()> {
        let direction = if pressed {
            Direction::Press
        } else {
            Direction::Release
        };
        
        // Map common keys
        let key = match key_str {
            "Return" | "Enter" => Key::Return,
            "Tab" => Key::Tab,
            "Space" | " " => Key::Space,
            "Escape" => Key::Escape,
            "BackSpace" => Key::Backspace,
            "Up" => Key::UpArrow,
            "Down" => Key::DownArrow,
            "Left" => Key::LeftArrow,
            "Right" => Key::RightArrow,
            _ => {
                // For single characters, use Unicode
                if let Some(ch) = key_str.chars().next() {
                    Key::Unicode(ch)
                } else {
                    return Ok(());
                }
            }
        };
        
        self.enigo.key(key, direction)?;
        
        Ok(())
    }
}