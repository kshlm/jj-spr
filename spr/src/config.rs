/*
 * Copyright (c) Radical HQ Limited
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;

use crate::{error::Result, github::GitHubBranch, utils::slugify};

#[derive(Clone, Debug)]
pub struct Config {
    pub owner: String,
    pub repo: String,
    pub remote_name: String,
    pub master_ref: GitHubBranch,
    pub branch_prefix: String,
    pub require_approval: bool,
    pub require_test_plan: bool,
    pub github_host: String,
    pub github_api_url: String,
}

impl Config {
    pub fn new(
        owner: String,
        repo: String,
        remote_name: String,
        master_branch: String,
        branch_prefix: String,
        require_approval: bool,
        require_test_plan: bool,
        github_host: String,
    ) -> Self {
        let master_ref =
            GitHubBranch::new_from_branch_name(&master_branch, &remote_name, &master_branch);
        let github_api_url = Self::build_api_url(&github_host);
        Self {
            owner,
            repo,
            remote_name,
            master_ref,
            branch_prefix,
            require_approval,
            require_test_plan,
            github_host,
            github_api_url,
        }
    }

    pub fn build_api_url(github_host: &str) -> String {
        if github_host == "github.com" {
            "https://api.github.com".to_string()
        } else {
            format!("https://{}/api/v3", github_host)
        }
    }

    pub fn pull_request_url(&self, number: u64) -> String {
        format!(
            "https://{host}/{owner}/{repo}/pull/{number}",
            host = &self.github_host,
            owner = &self.owner,
            repo = &self.repo
        )
    }

    pub fn parse_pull_request_field(&self, text: &str) -> Option<u64> {
        if text.is_empty() {
            return None;
        }

        let regex = lazy_regex::regex!(r#"^\s*#?\s*(\d+)\s*$"#);
        let m = regex.captures(text);
        if let Some(caps) = m {
            return Some(caps.get(1).unwrap().as_str().parse().unwrap());
        }

        let pattern = format!(
            r#"^\s*https?://{}/([\w\-\.]+)/([\w\-\.]+)/pull/(\d+)([/?#].*)?\s*$"#,
            regex::escape(&self.github_host)
        );
        if let Ok(regex) = regex::Regex::new(&pattern) {
            if let Some(caps) = regex.captures(text) {
                if self.owner == caps.get(1).unwrap().as_str()
                    && self.repo == caps.get(2).unwrap().as_str()
                {
                    return Some(caps.get(3).unwrap().as_str().parse().unwrap());
                }
            }
        }

        None
    }

    pub fn get_new_branch_name(&self, existing_ref_names: &HashSet<String>, title: &str) -> String {
        self.find_unused_branch_name(existing_ref_names, &slugify(title))
    }

    pub fn get_base_branch_name(
        &self,
        existing_ref_names: &HashSet<String>,
        title: &str,
    ) -> String {
        self.find_unused_branch_name(
            existing_ref_names,
            &format!("{}.{}", self.master_ref.branch_name(), &slugify(title)),
        )
    }

    fn find_unused_branch_name(&self, existing_ref_names: &HashSet<String>, slug: &str) -> String {
        let remote_name = &self.remote_name;
        let branch_prefix = &self.branch_prefix;
        let mut branch_name = format!("{branch_prefix}{slug}");
        let mut suffix = 0;

        loop {
            let remote_ref = format!("refs/remotes/{remote_name}/{branch_name}");

            if !existing_ref_names.contains(&remote_ref) {
                return branch_name;
            }

            suffix += 1;
            branch_name = format!("{branch_prefix}{slug}-{suffix}");
        }
    }

    pub fn new_github_branch_from_ref(&self, ghref: &str) -> Result<GitHubBranch> {
        GitHubBranch::new_from_ref(ghref, &self.remote_name, self.master_ref.branch_name())
    }

    pub fn new_github_branch(&self, branch_name: &str) -> GitHubBranch {
        GitHubBranch::new_from_branch_name(
            branch_name,
            &self.remote_name,
            self.master_ref.branch_name(),
        )
    }
}

pub enum AuthTokenSource {
    Config(String),
    GitHubCLI(String),
}

impl AuthTokenSource {
    pub fn token(&self) -> &String {
        match self {
            AuthTokenSource::Config(token) | AuthTokenSource::GitHubCLI(token) => token,
        }
    }
}

pub fn get_auth_token(git_config: &git2::Config) -> Option<String> {
    get_auth_token_with_source(git_config).map(|v| v.token().to_owned())
}

pub fn get_auth_token_with_source(git_config: &git2::Config) -> Option<AuthTokenSource> {
    get_auth_token_with_source_for_host(git_config, "github.com")
}

pub fn get_auth_token_with_source_for_host(git_config: &git2::Config, github_host: &str) -> Option<AuthTokenSource> {
    // Prefer the configured token if it exists
    if let Some(token) = get_config_value("spr.githubAuthToken", git_config) {
        return Some(AuthTokenSource::Config(token));
    }

    // Try to get a token from the gh CLI for the specific host
    let output = std::process::Command::new("gh")
        .args(["auth", "token", "--hostname", github_host])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    if output.status.success() {
        Some(AuthTokenSource::GitHubCLI(
            String::from_utf8(output.stdout).ok()?.trim().to_owned(),
        ))
    } else {
        None
    }
}

// Helper function to get config value from jj first, then git
pub fn get_config_value(key: &str, git_config: &git2::Config) -> Option<String> {
    // Try jj config first
    if let Ok(output) = std::process::Command::new("jj")
        .args(["config", "get", key])
        .output()
        && output.status.success()
        && let Ok(value) = String::from_utf8(output.stdout)
    {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    // Fall back to git config
    git_config.get_string(key).ok()
}

pub fn get_config_bool(key: &str, git_config: &git2::Config) -> Option<bool> {
    // Try jj config first
    if let Ok(output) = std::process::Command::new("jj")
        .args(["config", "get", key])
        .output()
        && output.status.success()
        && let Ok(value) = String::from_utf8(output.stdout)
    {
        let trimmed = value.trim().to_lowercase();
        if trimmed == "true" {
            return Some(true);
        } else if trimmed == "false" {
            return Some(false);
        }
    }

    // Fall back to git config
    git_config.get_bool(key).ok()
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    fn config_factory() -> Config {
        crate::config::Config::new(
            "acme".into(),
            "codez".into(),
            "origin".into(),
            "master".into(),
            "spr/foo/".into(),
            false,
            true,
            "github.com".into(),
        )
    }

    fn config_factory_enterprise() -> Config {
        crate::config::Config::new(
            "acme".into(),
            "codez".into(),
            "origin".into(),
            "master".into(),
            "spr/foo/".into(),
            false,
            true,
            "github.company.com".into(),
        )
    }

    #[test]
    fn test_pull_request_url() {
        let gh = config_factory();

        assert_eq!(
            &gh.pull_request_url(123),
            "https://github.com/acme/codez/pull/123"
        );
    }

    #[test]
    fn test_parse_pull_request_field_empty() {
        let gh = config_factory();

        assert_eq!(gh.parse_pull_request_field(""), None);
        assert_eq!(gh.parse_pull_request_field("   "), None);
        assert_eq!(gh.parse_pull_request_field("\n"), None);
    }

    #[test]
    fn test_parse_pull_request_field_number() {
        let gh = config_factory();

        assert_eq!(gh.parse_pull_request_field("123"), Some(123));
        assert_eq!(gh.parse_pull_request_field("   123 "), Some(123));
        assert_eq!(gh.parse_pull_request_field("#123"), Some(123));
        assert_eq!(gh.parse_pull_request_field(" # 123"), Some(123));
    }

    #[test]
    fn test_parse_pull_request_field_url() {
        let gh = config_factory();

        assert_eq!(
            gh.parse_pull_request_field("https://github.com/acme/codez/pull/123"),
            Some(123)
        );
        assert_eq!(
            gh.parse_pull_request_field("  https://github.com/acme/codez/pull/123  "),
            Some(123)
        );
        assert_eq!(
            gh.parse_pull_request_field("https://github.com/acme/codez/pull/123/"),
            Some(123)
        );
        assert_eq!(
            gh.parse_pull_request_field("https://github.com/acme/codez/pull/123?x=a"),
            Some(123)
        );
        assert_eq!(
            gh.parse_pull_request_field("https://github.com/acme/codez/pull/123/foo"),
            Some(123)
        );
        assert_eq!(
            gh.parse_pull_request_field("https://github.com/acme/codez/pull/123#abc"),
            Some(123)
        );
    }

    #[test]
    fn test_build_api_url_github_com() {
        assert_eq!(Config::build_api_url("github.com"), "https://api.github.com");
    }

    #[test]
    fn test_build_api_url_enterprise() {
        assert_eq!(
            Config::build_api_url("github.company.com"),
            "https://github.company.com/api/v3"
        );
    }

    #[test]
    fn test_pull_request_url_enterprise() {
        let gh = config_factory_enterprise();

        assert_eq!(
            &gh.pull_request_url(123),
            "https://github.company.com/acme/codez/pull/123"
        );
    }

    #[test]
    fn test_parse_pull_request_field_url_enterprise() {
        let gh = config_factory_enterprise();

        assert_eq!(
            gh.parse_pull_request_field("https://github.company.com/acme/codez/pull/123"),
            Some(123)
        );
        assert_eq!(
            gh.parse_pull_request_field("  https://github.company.com/acme/codez/pull/123  "),
            Some(123)
        );
        assert_eq!(
            gh.parse_pull_request_field("https://github.company.com/acme/codez/pull/123/"),
            Some(123)
        );
        assert_eq!(
            gh.parse_pull_request_field("https://github.company.com/acme/codez/pull/456?x=a"),
            Some(456)
        );
    }

    #[test]
    fn test_parse_pull_request_field_rejects_wrong_host() {
        let gh = config_factory_enterprise();

        assert_eq!(
            gh.parse_pull_request_field("https://github.com/acme/codez/pull/123"),
            None,
            "Should reject github.com URL when configured for enterprise"
        );
    }

    #[test]
    fn test_parse_pull_request_field_rejects_wrong_owner() {
        let gh = config_factory_enterprise();

        assert_eq!(
            gh.parse_pull_request_field("https://github.company.com/different/codez/pull/123"),
            None,
            "Should reject PR from different owner"
        );
    }

    #[test]
    fn test_parse_pull_request_field_rejects_wrong_repo() {
        let gh = config_factory_enterprise();

        assert_eq!(
            gh.parse_pull_request_field("https://github.company.com/acme/different/pull/123"),
            None,
            "Should reject PR from different repo"
        );
    }

    #[test]
    fn test_parse_pull_request_field_enterprise_with_subdomain() {
        let gh = crate::config::Config::new(
            "owner".into(),
            "repo".into(),
            "origin".into(),
            "main".into(),
            "spr/test/".into(),
            false,
            false,
            "github.internal.company.com".into(),
        );

        assert_eq!(
            gh.parse_pull_request_field("https://github.internal.company.com/owner/repo/pull/789"),
            Some(789),
            "Should handle subdomains in GitHub Enterprise host"
        );
    }

    #[test]
    fn test_build_api_url_with_subdomains() {
        assert_eq!(
            Config::build_api_url("github.internal.company.com"),
            "https://github.internal.company.com/api/v3"
        );
    }

    #[test]
    fn test_pull_request_url_github_com() {
        let gh = config_factory();
        assert_eq!(
            &gh.pull_request_url(456),
            "https://github.com/acme/codez/pull/456"
        );
    }

    #[test]
    fn test_github_host_field_is_set() {
        let gh = config_factory();
        assert_eq!(gh.github_host, "github.com");

        let gh_enterprise = config_factory_enterprise();
        assert_eq!(gh_enterprise.github_host, "github.company.com");
    }

    #[test]
    fn test_github_api_url_field_is_set() {
        let gh = config_factory();
        assert_eq!(gh.github_api_url, "https://api.github.com");

        let gh_enterprise = config_factory_enterprise();
        assert_eq!(
            gh_enterprise.github_api_url,
            "https://github.company.com/api/v3"
        );
    }
}
