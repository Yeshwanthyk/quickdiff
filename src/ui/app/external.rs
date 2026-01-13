use super::App;
use arboard::Clipboard;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use shell_words::split;
use std::{
    env,
    io::{self, Write},
    process::Command,
};

impl App {
    /// Open the currently selected file in the external editor.
    pub fn open_selected_in_editor(&mut self) {
        let Some(file) = self.selected_file() else {
            self.ui.error = Some("No file selected to open".to_string());
            self.ui.dirty = true;
            return;
        };

        let path = file.path.to_absolute(&self.repo);
        let command_parts = match Self::editor_command() {
            Ok(parts) => parts,
            Err(msg) => {
                self.ui.error = Some(msg);
                self.ui.dirty = true;
                return;
            }
        };

        let (program, args) = command_parts
            .split_first()
            .expect("editor command is non-empty");
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd.arg(&path);

        if let Err(e) = Self::suspend_terminal_for_external() {
            self.ui.error = Some(format!("Failed to release terminal: {}", e));
            self.ui.dirty = true;
            return;
        }

        let status = cmd.status();

        if let Err(e) = Self::resume_terminal_after_external() {
            self.ui.error = Some(format!("Failed to restore terminal: {}", e));
            self.ui.dirty = true;
            return;
        }

        match status {
            Ok(status) => {
                if status.success() {
                    self.ui.status = Some(format!("Editor closed for {}", file.path.as_str()));
                    self.ui.error = None;
                } else {
                    self.ui.error = Some(format!("Editor exited with code {:?}", status.code()));
                }
            }
            Err(e) => {
                self.ui.error = Some(format!("Failed to launch editor: {}", e));
            }
        }

        self.ui.dirty = true;
    }

    fn editor_command() -> Result<Vec<String>, String> {
        for key in ["QUICKDIFF_EDITOR", "VISUAL", "EDITOR"] {
            if let Ok(value) = env::var(key) {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    continue;
                }
                match split(trimmed) {
                    Ok(parts) if !parts.is_empty() => return Ok(parts),
                    Ok(_) => continue,
                    Err(e) => {
                        return Err(format!("Failed to parse ${}: {}", key, e));
                    }
                }
            }
        }
        Err("Set $QUICKDIFF_EDITOR, $VISUAL, or $EDITOR to open files externally".to_string())
    }

    fn suspend_terminal_for_external() -> io::Result<()> {
        disable_raw_mode()?;
        execute!(io::stdout(), DisableMouseCapture)?;
        execute!(io::stdout(), LeaveAlternateScreen)?;
        io::stdout().flush()?;
        Ok(())
    }

    fn resume_terminal_after_external() -> io::Result<()> {
        execute!(io::stdout(), EnterAlternateScreen)?;
        enable_raw_mode()?;
        execute!(io::stdout(), EnableMouseCapture)?;
        io::stdout().flush()?;
        Ok(())
    }

    /// Copy the selected file path to the clipboard.
    pub fn copy_selected_path(&mut self) {
        let Some(file) = self.selected_file() else {
            self.ui.error = Some("No file selected to copy".to_string());
            self.ui.dirty = true;
            return;
        };

        match Clipboard::new() {
            Ok(mut clipboard) => {
                if let Err(e) = clipboard.set_text(file.path.as_str().to_string()) {
                    self.ui.error = Some(format!("Clipboard error: {}", e));
                } else {
                    self.ui.status = Some(format!("Copied {} to clipboard", file.path));
                }
            }
            Err(e) => {
                self.ui.error = Some(format!("Clipboard unavailable: {}", e));
            }
        }

        self.ui.dirty = true;
    }
}
