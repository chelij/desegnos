//! The one list of what counts as a credential, for provider messages, diagnostic replies and crash reports.

pub const TOKEN_PREFIXES: [&str; 5] = ["sk-", "ghp_", "gho_", "github_pat_", "AIza"];

/// Names that introduce a credential: `name=value` or `name value`.
pub const SECRET_NAMES: [&str; 10] = [
    "bearer",
    "authorization",
    "api_key",
    "apikey",
    "access_token",
    "accesstoken",
    "refresh_token",
    "refreshtoken",
    "client_secret",
    "password",
];

fn is_jwt(word: &str) -> bool {
    word.starts_with("eyJ") && word.matches('.').count() >= 2
}

/// A whole word that is itself a credential.
pub fn is_secret_word(word: &str) -> bool {
    TOKEN_PREFIXES.iter().any(|prefix| word.starts_with(prefix)) || is_jwt(word)
}

/// Text that mentions a credential anywhere, even inside a longer word.
pub fn contains_secret(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    SECRET_NAMES.iter().any(|name| lower.contains(name))
        || TOKEN_PREFIXES.iter().any(|prefix| text.contains(prefix))
        || text.split_whitespace().any(is_jwt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_credential_shape_is_recognised() {
        for word in [
            "sk-abc",
            "ghp_abc",
            "gho_abc",
            "github_pat_abc",
            "AIzaabc",
            "eyJh.p.s",
        ] {
            assert!(is_secret_word(word), "{word}");
            assert!(contains_secret(&format!("before {word} after")), "{word}");
        }
        for text in [
            "Bearer x",
            "Authorization: x",
            "api_key=x",
            "apikey x",
            "access_token=x",
            "accessToken x",
            "refresh_token=x",
            "refreshToken x",
            "client_secret=x",
            "password=x",
        ] {
            assert!(contains_secret(text), "{text}");
        }
        assert!(!is_secret_word("ordinary"));
        assert!(!contains_secret("The model is overloaded. Try again."));
    }
}
