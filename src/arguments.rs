use clap::Parser;

#[derive(Debug, Clone, Parser)]
pub struct Arguments {
	#[clap(long, help = "The username for the git account")]
	pub git_username: String,
	#[clap(long, help = "The branch for the git branch")]
	pub git_email: Option<String>,
	#[clap(long, help = "The auth token for the git account")]
	pub git_auth_token: String,
	#[clap(long, help = "The url for the git repository")]
	pub git_url: String,
}