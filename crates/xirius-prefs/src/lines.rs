//! Recover independent, single-line assignments without overwriting the original file.

pub fn keymap(text: &str, problems: &mut Vec<String>) -> crate::Keymap {
    if let Ok(table) = text.parse::<toml::Table>() {
        return crate::Keymap::from_toml(&table, problems);
    }
    let mut section = String::new();
    let mut section_valid = true;
    let mut accepted = String::new();
    let mut duplicates = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.starts_with('[') {
            if format!("{t}\n").parse::<toml::Table>().is_ok() {
                section = t.to_string();
                section_valid = true;
            } else {
                section_valid = false;
                problems.push(format!("keymap.toml:{}: {line}", n + 1));
            }
            continue;
        }
        if !section_valid {
            problems.push(format!("keymap.toml:{}: {line}", n + 1));
            continue;
        }
        // Reopening a TOML section is invalid. Keep each assignment independently instead.
        let one = format!("{section}\n{line}\n");
        match one.parse::<toml::Table>() {
            Ok(table) => {
                // Convert sections to dotted assignments so every line stays independent.
                if section.is_empty() {
                    let trial = format!("{accepted}{line}\n");
                    if trial.parse::<toml::Table>().is_ok() {
                        accepted = trial;
                    } else {
                        problems.push(format!("keymap.toml:{}: {line}", n + 1));
                    }
                } else {
                    let section_key = section.trim_start_matches('[').trim_end_matches(']');
                    let dotted = format!("{section_key}.{line}\n");
                    let mut trial = accepted.clone();
                    trial.push_str(&dotted);
                    if trial.parse::<toml::Table>().is_ok() {
                        accepted = trial;
                    } else {
                        let binding = crate::Keymap::from_toml(&table, &mut Vec::new());
                        duplicates.push(binding.add);
                        problems.push(format!("keymap.toml:{}: {line}", n + 1));
                    }
                }
            }
            Err(_) => problems.push(format!("keymap.toml:{}: {line}", n + 1)),
        }
    }
    let mut map = crate::Keymap::from_toml(&accepted.parse().unwrap_or_default(), problems);
    for duplicate in duplicates {
        for (context, rows) in duplicate {
            for (key, commands) in rows {
                map.add
                    .entry(context.clone())
                    .or_default()
                    .entry(key)
                    .or_default()
                    .extend(commands);
            }
        }
    }
    map
}
