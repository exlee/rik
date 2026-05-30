use std::sync::OnceLock;

static MOTD_QUOTES: OnceLock<Vec<&'static str>> = OnceLock::new();
static PREWORK_QUOTES: OnceLock<Vec<&'static str>> = OnceLock::new();
static POSTWORK_QUOTES: OnceLock<Vec<&'static str>> = OnceLock::new();
static TOOL_PAD: &str = "    ";

pub fn motd_personality() {
    let quotes = MOTD_QUOTES.get_or_init(|| {
        include_str!("../safety.txt")
            .lines()
            .map(|s| s.trim())
            .collect::<Vec<_>>()
    });

    let pick_idx = rand::random_range(0..quotes.len());
    let pick = quotes[pick_idx];

    let wrapped = {
        static WRAP_COLUMN: usize = 32;
        static MAX_SIZE: usize = 35;
        let words: Vec<&str> = pick.split(' ').collect();
        let mut line_len = 0;
        let mut lines: Vec<String> = Vec::new();
        let mut cur_line = String::new();
        for (i, word) in words.iter().enumerate() {
            if i == 0 {
                cur_line.push_str(word);
                line_len = word.len();
            } else if line_len + 1 + word.len() <= WRAP_COLUMN {
                cur_line.push(' ');
                cur_line.push_str(word);
                line_len += 1 + word.len();
            } else {
                lines.push(cur_line.clone());
                cur_line.clear();
                cur_line.push_str(word);
                line_len = word.len();
            }
        }
        lines.push(cur_line.clone());

        fn pad_line(line: &str) -> String {
            let pad_size = MAX_SIZE.saturating_sub(line.len());
            let left = if pad_size % 2 == 0 {
                " ".repeat(pad_size / 2)
            } else {
                " ".repeat((pad_size + 1) / 2)
            };

            format!("{}{}\n", left, line)
        }

        let mut result = String::new();
        for line in lines.iter() {
            result.push_str(&pad_line(line));
        }

        let result = result.trim_end().to_string();
        result
    };
    println!("\n{}", wrapped);
}
pub fn pre_work_personality(alias: &str) {
    let quotes = PREWORK_QUOTES.get_or_init(|| {
        include_str!("../pre_work.txt")
            .lines()
            .map(|s| s.trim())
            .collect::<Vec<_>>()
    });
    let alias_string = String::from(alias);
    let pick_idx = rand::random_range(0..quotes.len());
    let pick = quotes[pick_idx];
    println!("{TOOL_PAD}[{}] {}", alias_string, pick);
}
pub fn post_work_personality(alias: &str) {
    let quotes = POSTWORK_QUOTES.get_or_init(|| {
        include_str!("../post_work.txt")
            .lines()
            .map(|s| s.trim())
            .collect::<Vec<_>>()
    });
    let pick_idx = rand::random_range(0..quotes.len());
    let pick = quotes[pick_idx];
    println!("[{}] {}", alias, pick);
}
