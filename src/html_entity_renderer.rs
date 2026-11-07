pub trait HtmlEntityRenderer: Send + Sync {
    fn render(&self, html: &str) -> String;
}

#[derive(Debug, Default)]
pub struct SimpleHtmlEntityRenderer {}

impl HtmlEntityRenderer for SimpleHtmlEntityRenderer {
    fn render(&self, html: &str) -> String {
        let mut result = String::new();
        let html = html.chars().collect::<Vec<_>>();
        let mut index = 0;

        loop {
            if index >= html.len() {
                break;
            }

            if is_html_entity(&html, index) {
                let (new_index, replacement) = replace_html_entity(&html, index);
                index = new_index;
                if let Some(replacement) = replacement {
                    result.push_str(&replacement);
                } else {
                    result.push('&');
                }

                continue;
            }
            result.push(html[index]);
            index += 1;
        }

        return result;
    }
}

fn is_html_entity(text: &[char], index: usize) -> bool {
    if index >= text.len() || text[index] != '&' {
        return false;
    }

    match text.get(index + 1) {
        Some('#') => true,
        Some(ch) => ch.is_ascii_alphabetic(),
        None => false,
    }
}

fn replace_html_entity(text: &[char], index: usize) -> (usize, Option<String>) {
    let (new_index, entity) = extract_html_entity(text, index);

    if let Some(entity) = entity {
        match entity {
            HtmlEntity::Named(name) => {
                let replacement = match name.as_ref() {
                    "&nbsp;" | "&emsp;" | "&thinsp;" => " ",
                    "&rarr;" => "→",
                    "&mdash;" => "—",
                    "&amp;" => "&",
                    "&ndash;" => "–",
                    "&quot;" => "\"",
                    "&lt;" => "<",
                    "&gt;" => ">",
                    "&hellip;" => "…",
                    "&deg;" => "°",
                    "&minus;" => "-",
                    "&equals;" => "=",
                    "&times;" => "×",
                    "&Delta;" => "Δ",
                    unknown => unknown,
                }
                .to_string();
                (new_index, Some(replacement))
            }
            HtmlEntity::Unicode(replacement) => (new_index, Some(replacement.to_string())),
        }
    } else {
        (new_index, None)
    }
}

enum HtmlEntity {
    Named(String),
    Unicode(String),
}

fn extract_html_entity(text: &[char], index: usize) -> (usize, Option<HtmlEntity>) {
    let mut new_index = index;
    let mut named_buffer = String::new();
    let mut number_buffer = String::new();
    let mut is_numeric = false;
    let mut valid = false;
    let mut is_hex = false;

    loop {
        if new_index >= text.len() {
            break;
        }
        match text[new_index] {
            '&' => {
                named_buffer.push('&');
            }
            '#' => {
                is_numeric = true;

                if let Some(hex_marker) = text.get(new_index + 1)
                    && (*hex_marker == 'x' || *hex_marker == 'X')
                {
                    is_hex = true;
                    new_index += 1;
                }
            }
            ';' => {
                new_index += 1;
                named_buffer.push(';');
                valid = if is_numeric {
                    !number_buffer.is_empty()
                } else {
                    named_buffer.len() >= 2
                };

                break;
            }
            ch => {
                if (is_numeric && !(ch.is_ascii_digit() || (is_hex && ch.is_ascii_hexdigit())))
                    || !ch.is_ascii_alphanumeric()
                {
                    break;
                }

                if is_numeric {
                    number_buffer.push(ch);
                } else {
                    named_buffer.push(ch);
                }
            }
        }
        new_index += 1;
    }

    if valid {
        let entity = if is_numeric {
            let decoded = parse_u32(&number_buffer, is_hex)
                .and_then(char::from_u32)
                .filter(|ch| *ch != '\0')
                .map(String::from)
                .unwrap_or_default();

            HtmlEntity::Unicode(decoded)
        } else {
            HtmlEntity::Named(named_buffer.to_string())
        };

        (new_index, Some(entity))
    } else {
        (index + 1, None)
    }
}

fn parse_u32(value: &str, is_hex: bool) -> Option<u32> {
    let radix = if is_hex { 16 } else { 10 };

    match u32::from_str_radix(value, radix) {
        Ok(number) => Some(number),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests;
