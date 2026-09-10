use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "mimic",
    version = "4.0.0",
    author = "Mimic Lotus <dev@mimicos.org>",
    about = "👑 Unified Package Engine & System Intelligence for Arch Linux"
)]
pub struct Cli {
    /// Specify an alternate root directory for package operations (Sandbox Mode)
    #[arg(short = 'r', long = "root", global = true, value_name = "PATH")]
    pub root: Option<String>,

    /// Specify an alternate database location (Sandbox Mode)
    #[arg(short = 'b', long = "dbpath", global = true, value_name = "PATH")]
    pub dbpath: Option<String>,

    /// Specify an alternate configuration file path
    #[arg(long = "config", global = true, value_name = "PATH")]
    pub config: Option<String>,

    /// Short alias to perform a full system upgrade
    #[arg(short = 'u', long = "upgrade")]
    pub upgrade: bool,

    /// Disable automatic CachyOS CPU-optimized repository injection
    #[arg(long = "no-cachy", global = true)]
    pub no_cachy: bool,

    /// Add custom GitHub micro-repository (owner/repo)
    #[arg(long = "micro-repo", global = true, value_name = "OWNER/REPO")]
    pub micro_repos: Vec<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum Commands {
    /// 🔄 Synchronize package databases from mirrors (-Sy)
    #[command(alias = "-Sy", alias = "-Syy")]
    Sync {
        /// Force refresh all package databases even if up to date (-yy)
        #[arg(short = 'y', long = "refresh")]
        refresh: bool,
    },
    /// 🚀 Synchronize databases and perform a full system upgrade (-Syu)
    #[command(alias = "up", alias = "update", alias = "-Syu", alias = "-Syyu")]
    Upgrade {
        /// Force refresh all package databases even if up to date (-yy)
        #[arg(short = 'y', long = "refresh")]
        refresh: bool,

        /// Do not prompt for confirmation
        #[arg(long = "noconfirm")]
        noconfirm: bool,
    },
    /// 🔍 Search across official mirrors, micro-repos, and AUR (-Ss)
    #[command(alias = "find", alias = "-Ss")]
    Search {
        /// Target package query or keyword
        query: String,

        /// Search only the AUR
        #[arg(long = "aur")]
        aur_only: bool,
    },
    /// ℹ️ Display detailed information about a package (-Si / -Qi)
    #[command(alias = "-Si", alias = "-Qi")]
    Info {
        /// Target package name
        package: String,
    },
    /// 📦 Install packages from official mirrors, micro-repos, or AUR (-S)
    #[command(alias = "add", alias = "get", alias = "-S")]
    Install {
        /// Target packages to install
        #[arg(required = true)]
        packages: Vec<String>,

        /// Do not prompt for confirmation
        #[arg(short = 'y', long = "noconfirm")]
        noconfirm: bool,
    },
    /// 🔨 Build an AUR package inside a hermetic bwrap container
    #[command(alias = "make", alias = "compile", alias = "-B")]
    Build {
        /// Target AUR package name
        package: String,

        /// Output directory for built package artifacts
        #[arg(short = 'o', long = "output")]
        output: Option<String>,

        /// Install after successful build
        #[arg(short = 'i', long = "install")]
        install: bool,

        /// Do not prompt for confirmation
        #[arg(short = 'y', long = "noconfirm")]
        noconfirm: bool,
    },
    /// 🐙 Build and package directly from a Git source repository
    #[command(alias = "git-build", alias = "from-git")]
    Git {
        /// Git repository URL (e.g. https://github.com/user/repo)
        url: String,

        /// Optional Git branch or tag to build
        #[arg(long = "branch")]
        branch: Option<String>,

        /// Do not prompt for confirmation before installing
        #[arg(short = 'y', long = "noconfirm")]
        noconfirm: bool,

        /// Build only without installing
        #[arg(long = "no-install")]
        no_install: bool,
    },
    /// 🗑️ Remove packages from the system (-R / -Rns)
    #[command(alias = "uninstall", alias = "rm", alias = "-R", alias = "-Rns")]
    Remove {
        /// Target packages to remove
        #[arg(required = true)]
        packages: Vec<String>,

        /// Remove all unneeded dependencies (cascade)
        #[arg(short = 'c', long = "cascade")]
        cascade: bool,

        /// Do not prompt for confirmation
        #[arg(short = 'y', long = "noconfirm")]
        noconfirm: bool,
    },
    /// 🧠 Ask the dormant AI mentor why a package is needed or how it works
    Why {
        /// Package name or natural language query
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
    },
    /// 🔑 Manage pacman trusted GPG keyrings
    #[command(subcommand, alias = "keyring")]
    Key(KeyCommands),
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum KeyCommands {
    /// 🔑 Initialize the system GPG trust database
    Init,
    /// 📥 Populate trusted keys from /usr/share/pacman/keyrings/
    Populate {
        /// Specific keyrings to populate (e.g., archlinux, cachyos, mimicos). Defaults to all found.
        #[arg(value_name = "KEYRING")]
        names: Vec<String>,
    },
    /// 🔄 Synchronize keyring packages, repopulate, and refresh from keyserver
    Sync,
    /// 🔍 List trusted signing keys
    List,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parse_sync() {
        let cli = Cli::try_parse_from(["mimic", "sync"]).expect("Failed to parse sync");
        assert_eq!(cli.command, Some(Commands::Sync { refresh: false }));
        assert!(!cli.upgrade);
    }

    #[test]
    fn test_cli_parse_sync_refresh() {
        let cli = Cli::try_parse_from(["mimic", "sync", "-y"]).expect("Failed to parse sync -y");
        assert_eq!(cli.command, Some(Commands::Sync { refresh: true }));
        assert!(!cli.upgrade);
    }

    #[test]
    fn test_cli_parse_sync_alias() {
        let cli = Cli::try_parse_from(["mimic", "-Sy"]).expect("Failed to parse -Sy");
        assert_eq!(cli.command, Some(Commands::Sync { refresh: false }));
        assert!(!cli.upgrade);
    }

    #[test]
    fn test_cli_parse_upgrade() {
        let cli = Cli::try_parse_from(["mimic", "upgrade"]).expect("Failed to parse upgrade");
        assert_eq!(cli.command, Some(Commands::Upgrade { refresh: false, noconfirm: false }));
        assert!(!cli.upgrade);
    }

    #[test]
    fn test_cli_parse_upgrade_flags() {
        let cli = Cli::try_parse_from(["mimic", "upgrade", "-y", "--noconfirm"]).expect("Failed to parse upgrade flags");
        assert_eq!(cli.command, Some(Commands::Upgrade { refresh: true, noconfirm: true }));
        assert!(!cli.upgrade);
    }

    #[test]
    fn test_cli_parse_upgrade_alias_syu() {
        let cli = Cli::try_parse_from(["mimic", "-Syu"]).expect("Failed to parse -Syu");
        assert_eq!(cli.command, Some(Commands::Upgrade { refresh: false, noconfirm: false }));
        assert!(!cli.upgrade);
    }

    #[test]
    fn test_cli_parse_upgrade_short_flag() {
        let cli = Cli::try_parse_from(["mimic", "-u"]).expect("Failed to parse -u");
        assert!(cli.upgrade);
        assert_eq!(cli.command, None);
    }

    #[test]
    fn test_cli_parse_upgrade_long_flag() {
        let cli = Cli::try_parse_from(["mimic", "--upgrade"]).expect("Failed to parse --upgrade");
        assert!(cli.upgrade);
        assert_eq!(cli.command, None);
    }

    #[test]
    fn test_cli_parse_root_bare() {
        let cli = Cli::try_parse_from(["mimic"]).expect("Failed to parse bare mimic");
        assert!(!cli.upgrade);
        assert_eq!(cli.command, None);
    }

    #[test]
    fn test_cli_parse_why_single_word() {
        let cli = Cli::try_parse_from(["mimic", "why", "neovim"]).expect("Failed to parse why");
        assert_eq!(cli.command, Some(Commands::Why { query: vec!["neovim".to_string()] }));
    }

    #[test]
    fn test_cli_parse_why_multi_word() {
        let cli = Cli::try_parse_from(["mimic", "why", "how", "does", "pipewire", "work"]).expect("Failed to parse multi-word why");
        assert_eq!(
            cli.command,
            Some(Commands::Why {
                query: vec![
                    "how".to_string(),
                    "does".to_string(),
                    "pipewire".to_string(),
                    "work".to_string()
                ]
            })
        );
    }

    #[test]
    fn test_cli_parse_key_init() {
        let cli = Cli::try_parse_from(["mimic", "key", "init"]).expect("Failed to parse key init");
        assert_eq!(cli.command, Some(Commands::Key(KeyCommands::Init)));
    }

    #[test]
    fn test_cli_parse_key_populate_default() {
        let cli = Cli::try_parse_from(["mimic", "key", "populate"]).expect("Failed to parse key populate");
        assert_eq!(cli.command, Some(Commands::Key(KeyCommands::Populate { names: vec![] })));
    }

    #[test]
    fn test_cli_parse_key_populate_specific() {
        let cli = Cli::try_parse_from(["mimic", "key", "populate", "archlinux", "cachyos", "mimicos"]).expect("Failed to parse key populate specific");
        assert_eq!(
            cli.command,
            Some(Commands::Key(KeyCommands::Populate {
                names: vec!["archlinux".to_string(), "cachyos".to_string(), "mimicos".to_string()]
            }))
        );
    }

    #[test]
    fn test_cli_parse_key_sync() {
        let cli = Cli::try_parse_from(["mimic", "key", "sync"]).expect("Failed to parse key sync");
        assert_eq!(cli.command, Some(Commands::Key(KeyCommands::Sync)));
    }

    #[test]
    fn test_cli_parse_key_list() {
        let cli = Cli::try_parse_from(["mimic", "key", "list"]).expect("Failed to parse key list");
        assert_eq!(cli.command, Some(Commands::Key(KeyCommands::List)));
    }

    #[test]
    fn test_cli_parse_keyring_alias() {
        let cli = Cli::try_parse_from(["mimic", "keyring", "list"]).expect("Failed to parse keyring alias");
        assert_eq!(cli.command, Some(Commands::Key(KeyCommands::List)));
    }
}
