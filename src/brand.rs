//! Product naming, in one place.
//!
//! "Sift Mail" is the formal name: installer, Start menu, window title and the
//! About screen. "Sift" is the title-bar wordmark and short in-app copy.
//! Onboarding says "Welcome to Sift Mail" once and "Sift" after that.

/// Formal product name for the installer, Start menu, window title and About screen.
pub const PRODUCT_NAME: &str = "Sift Mail";

/// Short name for the title-bar wordmark and in-app references.
pub const SHORT_NAME: &str = "Sift";

/// First line of the onboarding flow, the only place that uses the formal name in-app.
pub const WELCOME_LINE: &str = "Welcome to Sift Mail";

/// Name of the shipped executable, without the platform extension.
pub const EXECUTABLE_NAME: &str = "SiftMail";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_line_uses_the_formal_name() {
        assert!(WELCOME_LINE.ends_with(PRODUCT_NAME));
    }

    #[test]
    fn short_name_is_the_first_word_of_the_product_name() {
        assert_eq!(PRODUCT_NAME.split_whitespace().next(), Some(SHORT_NAME));
    }

    #[test]
    fn executable_name_has_no_spaces() {
        assert!(!EXECUTABLE_NAME.contains(' '));
        assert_eq!(EXECUTABLE_NAME, PRODUCT_NAME.replace(' ', ""));
    }
}
