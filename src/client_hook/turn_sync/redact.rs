use crate::session::MAX_SESSION_TEXT_BYTES;

const REDACTED: &str = "[redacted]";

/// Selectively remove private spans and common credential forms before any local write.
pub(super) fn text(input: &str, bearer: &str) -> Option<String> {
    if input.len() > MAX_SESSION_TEXT_BYTES {
        return None;
    }
    let public = public_text(input);
    if public.trim().is_empty() || public.trim() == REDACTED {
        return None;
    }
    let public = public.replace(bearer, REDACTED);
    let mut out = String::new();
    let mut pem = false;
    for line in public.split_inclusive('\n') {
        let lower = line.to_ascii_lowercase();
        if lower.contains("-----begin ") && lower.contains("private key-----") {
            pem = true;
            out.push_str(REDACTED);
            out.push('\n');
        } else if !pem && !credential(&lower) {
            out.push_str(line);
        } else if !pem {
            out.push_str(REDACTED);
            if line.ends_with('\n') {
                out.push('\n');
            }
        }
        if pem && lower.contains("-----end ") && lower.contains("private key-----") {
            pem = false;
        }
    }
    (out.len() <= MAX_SESSION_TEXT_BYTES && !out.trim().is_empty()).then_some(out)
}

fn public_text(input: &str) -> String {
    let normalized = input.to_ascii_lowercase();
    let mut out = String::new();
    let mut offset = 0;
    let mut depth = 0;
    for (index, _) in normalized.match_indices('<') {
        let tail = &normalized[index..];
        if let Some(rest) = tail.strip_prefix("<private")
            && rest.starts_with(|c: char| c == '>' || c.is_ascii_whitespace())
        {
            if depth == 0 {
                out.push_str(&input[offset..index]);
                out.push_str(REDACTED);
            }
            depth += 1;
        } else if tail.starts_with("</private>") && depth > 0 {
            depth -= 1;
            if depth == 0 {
                offset = index + "</private>".len();
            }
        }
    }
    if depth == 0 {
        out.push_str(&input[offset..]);
    }
    out
}

fn credential(line: &str) -> bool {
    if ["authorization: bearer ", "authorization: basic ", "bearer "]
        .iter()
        .any(|prefix| line.contains(prefix))
    {
        return true;
    }
    line.split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ',' | '(' | ')'))
        .any(|word| {
            if ["sk-", "ghp_", "gho_", "github_pat_", "akia", "xoxb-"]
                .iter()
                .any(|prefix| word.len() >= 16 && word.starts_with(prefix))
            {
                return true;
            }
            if let Some((_, url)) = word.split_once("://")
                && let Some((identity, _)) = url.split('/').next().and_then(|s| s.split_once('@'))
                && identity.contains(':')
            {
                return true;
            }
            false
        })
        || line.match_indices(['=', ':']).any(|(i, _)| {
            let name = line[..i]
                .trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`'))
                .rsplit(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
                .next()
                .unwrap_or_default();
            [
                "api_key",
                "api-key",
                "apikey",
                "access_token",
                "token",
                "password",
                "secret",
            ]
            .iter()
            .any(|label| {
                name == *label || name.strip_suffix(label).is_some_and(|p| p.ends_with('_'))
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_and_credential_content_is_filtered_before_storage() {
        let input = "你好 <PRIVATE note=\"x\">hide <private>nested</private> too</PRIVATE>\n                     OPENAI_API_KEY = raw-key\n                     profile sk-proj-abcdefghijklmnop\n                     Authorization: Bearer abc\n                     -----BEGIN OPENSSH PRIVATE KEY-----\n                     private-key-bytes\n                     -----END OPENSSH PRIVATE KEY-----\n                     More context active-oc-secret";
        let output = text(input, "active-oc-secret").unwrap();
        assert!(output.contains("你好 [redacted]"));
        assert!(output.contains("More context [redacted]"));
        for secret in [
            "hide",
            "nested",
            "too",
            "raw-key",
            "abcdefghijklmnop",
            "Bearer abc",
            "private-key-bytes",
            "active-oc-secret",
        ] {
            assert!(!output.contains(secret), "found {secret}");
        }
        assert_eq!(
            text(r#"{ "password": "raw-password" }"#, "token").as_deref(),
            Some(REDACTED)
        );
        assert!(text("<private>everything</private>", "token").is_none());
        assert!(text(" <private>unterminated", "token").is_none());
    }
}
