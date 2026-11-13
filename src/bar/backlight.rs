use std::fs::{self, File, OpenOptions};
use std::io::Write;

use anyhow::{Context, Result, anyhow};

const NAMES: [&str; 3] = ["228600000.dsi.0", "display-pipe", "appletb_backlight"];

pub struct Backlight {
    file: File,
    max: u32,
}

impl Backlight {
    pub fn open() -> Result<Self> {
        let dir = fs::read_dir("/sys/class/backlight")?
            .filter_map(|e| e.ok())
            .find(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                NAMES.iter().any(|n| name.contains(n))
            })
            .ok_or_else(|| anyhow!("no Touch Bar backlight found"))?
            .path();
        let max = fs::read_to_string(dir.join("max_brightness"))?
            .trim()
            .parse()?;
        let path = dir.join("brightness");
        let file = OpenOptions::new()
            .write(true)
            .open(&path)
            .with_context(|| format!("opening {} for writing", path.display()))?;
        Ok(Self { file, max })
    }

    pub fn max(&self) -> u32 {
        self.max
    }

    pub fn set(&mut self, value: u32) -> Result<()> {
        self.file
            .write_all(format!("{}\n", value.min(self.max)).as_bytes())?;
        Ok(())
    }
}
