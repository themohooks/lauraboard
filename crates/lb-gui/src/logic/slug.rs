//! Stable identifiers derived from human-readable names, including Cyrillic.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        let mapped = match c {
            'а' => "a",
            'б' => "b",
            'в' => "v",
            'г' => "g",
            'д' => "d",
            'е' | 'ё' | 'э' => "e",
            'ж' => "zh",
            'з' => "z",
            'и' | 'й' => "i",
            'к' => "k",
            'л' => "l",
            'м' => "m",
            'н' => "n",
            'о' => "o",
            'п' => "p",
            'р' => "r",
            'с' => "s",
            'т' => "t",
            'у' => "u",
            'ф' => "f",
            'х' => "kh",
            'ц' => "ts",
            'ч' => "ch",
            'ш' => "sh",
            'щ' => "shch",
            'ы' => "y",
            'ю' => "yu",
            'я' => "ya",
            'ь' | 'ъ' => "",
            _ => {
                if c.is_ascii_alphanumeric() {
                    out.push(c);
                } else if !out.is_empty() && !out.ends_with('-') {
                    out.push('-');
                }
                continue;
            }
        };
        out.push_str(mapped);
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_become_editable_ascii_identifiers() {
        assert_eq!(super::slug("  Новости 24! "), "novosti-24");
        assert_eq!(super::slug("Morning / News"), "morning-news");
    }
}
