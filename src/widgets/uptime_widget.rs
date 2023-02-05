use std::{
    fs::{self, File},
    io::{self, Write},
};

pub struct UptimeWidgetState {
    pub streak: u64,
}

impl Default for UptimeWidgetState {
    fn default() -> Self {
        let saved_days =
            fs::read_to_string("/home/felix/.config/bottom/days").unwrap_or_else(|_| {
                let mut file = File::create("/home/felix/.config/bottom/days").unwrap();
                let mut days = String::new();
                io::stdin().read_line(&mut days).unwrap();
                days.pop();
                file.write_all(days.as_bytes()).unwrap();
                days
            });
        Self {
            streak: saved_days.parse().unwrap_or(0),
        }
    }
}
