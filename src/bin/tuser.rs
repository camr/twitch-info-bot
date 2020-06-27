use lambda::handler_fn;
use log::{self, error};
use rusoto_secretsmanager::{GetSecretValueRequest, SecretsManager, SecretsManagerClient};
use rusoto_signature::region::Region;
use serde_derive::{Deserialize, Serialize};
use serde_json::Value;
use simple_error::bail;
use simple_logger;
use tokio;

type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Deserialize, Debug)]
struct Secrets {
    slack_token: String,
    twitch_client_id: String,
    twitch_client_secret: String,
    twitch_app_token: String,
}

#[derive(Deserialize)]
struct UserSearchRequest {
    token: String,
    text: String,
}

#[derive(Serialize)]
struct SlackMessage {
    response_type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    text: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    attachments: Vec<SlackAttachment>,
}

#[derive(Serialize)]
struct SlackAttachment {
    color: String,
    author_name: String,
    author_icon: String,
}

#[derive(Deserialize, Debug)]
struct TwitchUserResponse {
    data: Vec<TwitchUser>,
}

#[derive(Deserialize, Debug)]
struct TwitchUser {
    #[serde(rename = "type")]
    user_type: String,

    id: String,
    login: String,
    display_name: String,
    broadcaster_type: String,
    description: String,
    profile_image_url: String,
    offline_image_url: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    simple_logger::init_with_level(log::Level::Info).expect("Could not initiate logger");
    // let ctx = lambda::context();

    let func = handler_fn(search_for_users);
    lambda::run(func).await
}

async fn search_for_users(event: Value) -> Result<SlackMessage, Error> {
    let body = event.get("body").expect("No body data sent").as_str().expect("Body data not a string?");

    let req: UserSearchRequest = serde_json::from_str(&body).unwrap();
    if req.token == "" {
        error!("Slack command invoked with empty token");
        bail!("No Slack token provided");
    }

    let cl = SecretsManagerClient::new(Region::UsWest2);
    let resp = cl.get_secret_value(GetSecretValueRequest {
        secret_id: "prod/tuser".to_string(),
        version_id: None,
        version_stage: None,
    }).await?;

    let secrets_str = resp.secret_string.expect("Could not find secrets");
    let secrets: Secrets = serde_json::from_str(&secrets_str).unwrap();

    if req.token != secrets.slack_token {
        error!("Slack command invoked with incorrect slack token");
        bail!("Bad Slack token provided");
    }

    let url = generate_api_url(&req.text)?;

    let users_result = get_user_info(url, &secrets);
    match users_result {
        Ok(users) => Ok(SlackMessage {
            response_type: format!("in_channel"),
            text: "".to_string(),
            attachments: users,
        }),
        Err(_e) => Ok(SlackMessage {
            response_type: format!("in_channel"),
            text: format!("User lookup failed for {}", req.text),
            attachments: vec![],
        }),
    }
}

fn generate_api_url(text: &String) -> Result<String, Error> {
    let mut ids: Vec<String> = vec![];
    let mut logins: Vec<String> = vec![];

    let mut items = text.split_whitespace();
    while let Some(item) = items.next() {
        let trimmed = item.trim_matches(',');

        match trimmed.parse::<u32>() {
            Ok(_ok) => {
                ids.push(format!("id={}", trimmed));
            }
            Err(_e) => {
                logins.push(format!("login={}", trimmed));
            }
        }
    }

    if ids.len() == 0 && logins.len() == 0 {
        bail!("No valid Twitch usernames or IDs found");
    }

    let mut params: String = "".to_string();
    if ids.len() > 0 {
        params = format!("{}{}&", params, ids.join("&"));
    }
    if logins.len() > 0 {
        params = format!("{}{}", params, logins.join("&"));
    }

    Ok(format!("https://api.twitch.tv/helix/users?{}", params))
}

fn get_user_info(url: String, secrets: &Secrets) -> Result<Vec<SlackAttachment>, Error> {
    let client = reqwest::blocking::Client::new();
    let resp = client
        .get(&url)
        .header("Client-ID", secrets.twitch_client_id.clone())
        .header("Authorization", format!("Bearer {}", secrets.twitch_app_token))
        .send();

    match resp {
        Ok(data) => {
            if data.status() != 200 {
                error!("Twitch response error: {:#?}", data);
                bail!("Received non-200 response from Twitch");
            }

            match data.json::<TwitchUserResponse>() {
                Ok(arr) => Ok(arr
                    .data
                    .iter()
                    .map(|a| SlackAttachment {
                        color: "#73535ad".to_string(),
                        author_name: format!("{}: {}", a.display_name, a.id),
                        author_icon: a.profile_image_url.clone(),
                    })
                    .collect()),
                Err(e) => {
                    error!("{}", e);
                    bail!("Could non decode Twitch response");
                }
            }
        }
        Err(e) => {
            error!("{}", e);
            bail!("Request to Twitch failed");
        }
    }
}
