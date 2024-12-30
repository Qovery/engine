use url::Url;

pub struct LetsEncryptConfig {
    email_report: String,
    acme_url: Url,
}

impl LetsEncryptConfig {
    pub fn new(email_report: String, use_test_sandbox: bool) -> Self {
        LetsEncryptConfig {
            email_report,
            acme_url: LetsEncryptConfig::acme_url_for_given_usage(use_test_sandbox),
        }
    }
    pub fn email_report(&self) -> &str {
        &self.email_report
    }
    pub fn acme_url(&self) -> &Url {
        &self.acme_url
    }
    pub fn acme_url_for_given_usage(use_test_sandbox: bool) -> Url {
        match use_test_sandbox {
            true => Url::parse("https://acme-staging-v02.api.letsencrypt.org/directory")
                .expect("Error while trying to parse letsencrypt stagging URL"),
            false => Url::parse("https://acme-v02.api.letsencrypt.org/directory")
                .expect("Error while trying to parse letsencrypt prod URL"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::environment::models::third_parties::LetsEncryptConfig;
    use url::Url;

    #[test]
    fn test_lets_encrypt_config_acme_url_for_given_usage() {
        // execute & verify:
        // Test sandbox URL
        assert_eq!(
            LetsEncryptConfig::acme_url_for_given_usage(true),
            Url::parse("https://acme-staging-v02.api.letsencrypt.org/directory")
                .expect("Error while trying to parse letsencrypt stagging URL")
        );
        // Production URL
        assert_eq!(
            LetsEncryptConfig::acme_url_for_given_usage(false),
            Url::parse("https://acme-v02.api.letsencrypt.org/directory")
                .expect("Error while trying to parse letsencrypt prod URL")
        );
    }
}
