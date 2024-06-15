use std::collections::HashMap;

use frankenstein::SendDiceParams;
use kinode_process_lib::vfs::{create_drive, open_file, DirEntry, FileType, VfsAction, VfsRequest};
use kinode_process_lib::{
    await_message, call_init, get_blob, http, println, Address, Message, Request, Response,
};

use llm_interface::openai::*;
use stt_interface::*;
use telegram_interface::*;

mod structs;
use structs::*;

mod tg_api;

mod files;
use files::{BackupResponse, ClientRequest, ServerResponse};

wit_bindgen::generate!({
    path: "wit",
    world: "process",
});

fn handle_http_message(
    our: &Address,
    message: &Message,
    state: &mut Option<State>,
    pkgs: &HashMap<Pkg, Address>,
) -> anyhow::Result<()> {
    println!("handle http message");
    match message {
        Message::Request { ref body, .. } => handle_http_request(our, state, body, pkgs),
        Message::Response { .. } => Ok(()),
    }
}

fn handle_http_request(
    our: &Address,
    state: &mut Option<State>,
    body: &[u8],
    pkgs: &HashMap<Pkg, Address>,
) -> anyhow::Result<()> {
    println!("handle http request");
    let http_request = http::HttpServerRequest::from_bytes(body)?
        .request()
        .ok_or_else(|| anyhow::anyhow!("Failed to parse http request"))?;
    let path = http_request.path()?;
    println!("path: {:?}", path);
    let bytes = get_blob()
        .ok_or_else(|| anyhow::anyhow!("Failed to get blob"))?
        .bytes;
    match path.as_str() {
        "/status" => {
            println!("fetching status");
            fetch_status()
        }
        "/submit_config" => submit_config(our, &bytes, state, pkgs),
        "/notes" => {
            println!("fetching notes");
            fetch_notes()
        }
        "/import_notes" => import_notes(&bytes),
        _ => Ok(()),
    }
}

fn import_notes(body_bytes: &[u8]) -> anyhow::Result<()> {
    println!("IMPORTING NOTES");
    let directory: HashMap<String, String> =
        serde_json::from_slice::<HashMap<String, String>>(body_bytes)?;

    let mut dirs_created: Vec<String> = Vec::new();

    for (file_path, content) in directory.iter() {
        println!("file_path: {:?}", &file_path);

        let drive_path: &str = "/command_center:appattacc.os/files";
        let full_file_path = format!("{}/{}", drive_path, file_path);

        println!("file_path: {:?}", &full_file_path);

        let mut split_path: Vec<&str> = full_file_path
            .split("/")
            .filter(|s| !s.is_empty())
            .collect::<Vec<&str>>();
        split_path.pop();
        let dir_path = split_path.join("/");
        println!("dir_path: {:?}", dir_path);

        // not perfect, i.e. it will run create /this/dir even if /this/dir/here/ exists
        // because it doesnt check was already created when /this/dir/here was created
        if !dirs_created.contains(&dir_path) {
            println!("creating dir: {:?}", dir_path);
            let request = VfsRequest {
                path: format!("/{}", dir_path).to_string(),
                action: VfsAction::CreateDirAll,
            };
            let _message = Request::new()
                .target(("our", "vfs", "distro", "sys"))
                .body(serde_json::to_vec(&request)?)
                .send_and_await_response(5)?;
        }

        dirs_created.push(dir_path);

        println!("creating file at {:?}", &full_file_path);
        let file = open_file(&full_file_path, true, Some(5))?;

        println!("write content to file");
        file.write(content.as_bytes())?;
    }

    http::send_response(
        http::StatusCode::OK,
        Some(HashMap::from([(
            "Content-Type".to_string(),
            "application/json".to_string(),
        )])),
        b"{\"message\": \"success\"}".to_vec(),
    );

    Ok(())
}

fn fetch_status() -> anyhow::Result<()> {
    let state = State::fetch()
        .ok_or_else(|| anyhow::anyhow!("State being fetched for the first time (or failed)"))?;
    let config = &state.config;
    let response_body = serde_json::to_string(&config)?;
    http::send_response(
        http::StatusCode::OK,
        Some(HashMap::from([(
            "Content-Type".to_string(),
            "application/json".to_string(),
        )])),
        response_body.as_bytes().to_vec(),
    );
    Ok(())
}

fn fetch_notes() -> anyhow::Result<()> {
    let dir_entry: DirEntry = DirEntry {
        path: NOTES_PATH.to_string(),
        file_type: FileType::Directory,
    };

    let notes = files::read_nested_dir(dir_entry)?;

    // println!("notes: {:?}", notes);

    let response_body = serde_json::to_string(&notes)?;
    http::send_response(
        http::StatusCode::OK,
        Some(HashMap::from([(
            "Content-Type".to_string(),
            "application/json".to_string(),
        )])),
        response_body.as_bytes().to_vec(),
    );
    Ok(())
}

// also creates state if doesn't exist
fn submit_config(
    our: &Address,
    body_bytes: &[u8],
    state: &mut Option<State>,
    pkgs: &HashMap<Pkg, Address>,
) -> anyhow::Result<()> {
    let initial_config = serde_json::from_slice::<InitialConfig>(body_bytes)?;
    match state {
        Some(state_) => {
            println!("Modifying state to {:?}", initial_config);
            state_.config = initial_config;
        }
        None => {
            println!("Creating state {:?}", initial_config);
            *state = Some(State::new(our, initial_config));
        }
    }

    if let Some(ref mut state) = state {
        for (pkg, addr) in pkgs.iter() {
            println!("submit_config: matching pkg: {:?}", pkg);
            match pkg {
                Pkg::LLM => {
                    if let Some(openai_key) = &state.config.openai_key {
                        let req = serde_json::to_vec(&LLMRequest::RegisterOpenaiApiKey(
                            RegisterApiKeyRequest {
                                api_key: openai_key.clone(),
                            },
                        ))?;
                        let _ = Request::new()
                            .target(addr.clone())
                            .body(req)
                            .send_and_await_response(5)??;
                    }
                    if let Some(groq_key) = &state.config.groq_key {
                        let req = serde_json::to_vec(
                            &llm_interface::openai::LLMRequest::RegisterGroqApiKey(
                                RegisterApiKeyRequest {
                                    api_key: groq_key.clone(),
                                },
                            ),
                        )?;
                        let _ = Request::new()
                            .target(addr.clone())
                            .body(req)
                            .send_and_await_response(5)??;
                    }
                }
                Pkg::STT => {
                    if let Some(openai_key) = &state.config.openai_key {
                        let req =
                            serde_json::to_vec(&STTRequest::RegisterApiKey(openai_key.clone()))?;
                        let _ = Request::new()
                            .target(addr.clone())
                            .body(req)
                            .send_and_await_response(5)??;
                    }
                }
                Pkg::Telegram => {
                    if let Some(telegram_key) = &state.config.telegram_key {
                        let init = TgInitialize {
                            token: telegram_key.clone(),
                            params: None,
                        };
                        let req = serde_json::to_vec(&TgRequest::RegisterApiKey(init))?;
                        let _ = Request::new()
                            .target(addr.clone())
                            .body(req)
                            .send_and_await_response(5)??;
                    }
                }
            }
        }
        state.save();

        http::send_response(
            http::StatusCode::OK,
            Some(HashMap::from([(
                "Content-Type".to_string(),
                "application/json".to_string(),
            )])),
            b"{\"message\": \"success\"}".to_vec(),
        );
    }
    Ok(())
}

fn handle_message(
    our: &Address,
    state: &mut Option<State>,
    pkgs: &HashMap<Pkg, Address>,
) -> anyhow::Result<()> {
    println!("handle message");
    let message = await_message()?;
    println!("message: {:?}", &message);
    if message.source().node != our.node {
        match &message {
            // receiving backup request from client
            Message::Request { body, .. } => {
                let deserialized: ClientRequest = serde_json::from_slice::<ClientRequest>(body)?;
                println!("body: {:?}", deserialized);
                match deserialized {
                    ClientRequest::BackupRequest { node_id, size } => {
                        println!(
                            "received backup_request from {:?} for {} size",
                            message.source().node,
                            size
                        );
                        // TODO: add criterion here
                        let backup_response: Vec<u8> = serde_json::to_vec(
                            &ServerResponse::BackupResponse(BackupResponse::Confirm),
                        )?;
                        let _r = Response::new().body(backup_response).send();
                    }
                }
            }
            Message::Response { body, .. } => {
                println!("got response from somewhere");
                println!("message: {:?}", &message);
                let deserialized: ServerResponse = serde_json::from_slice::<ServerResponse>(body)?;
                match deserialized {
                    ServerResponse::BackupResponse(backup_response) => {
                        println!("received response from {:?}", message.source().node);
                        println!("response: {:?}", backup_response)
                    }
                    _ => {}
                }
            }
        }
        return Ok(());
    }
    println!("message source: {:?}", message.source());
    match message.source().process.to_string().as_str() {
        "http_server:distro:sys" | "http_client:distro:sys" => {
            handle_http_message(&our, &message, state, pkgs)
        }
        // TODO: filter for getting it from the ui, for now use terminal
        _ => match &message {
            // making backup request to server
            Message::Request { body, .. } => {
                println!("got message from somewhere");
                println!("message: {:?}", &message);
                let deserialized = serde_json::from_slice::<files::ClientRequest>(body)?;
                println!("body: {:?}", deserialized);
                match deserialized {
                    files::ClientRequest::BackupRequest { node_id, size } => {
                        let backup_request = serde_json::to_vec(&ClientRequest::BackupRequest {
                            node_id: node_id.clone(),
                            size: 0,
                        })?;

                        let _ = Request::to(Address::new(
                            node_id,
                            ("main", "command_center", "appattacc.os"),
                        ))
                        .expects_response(5)
                        .body(backup_request)
                        .send();
                    }
                }
                Ok(())
            }
            _ => return Ok(()),
        },
    }
}

const ICON: &str = include_str!("icon");
const NOTES_PATH: &str = "/command_center:appattacc.os/files";
call_init!(init);
fn init(our: Address) {
    let _ = http::serve_ui(
        &our,
        "ui",
        true,
        false,
        vec!["/", "/submit_config", "/status", "/notes", "/import_notes"],
    );

    let mut state = State::fetch();

    // add ourselves to the homepage
    Request::to(("our", "homepage", "homepage", "sys"))
        .body(
            serde_json::json!({
                "Add": {
                    "label": "Command Center",
                    "icon": ICON,
                    "path": "/", // just our root
                }
            })
            .to_string()
            .as_bytes()
            .to_vec(),
        )
        .send()
        .unwrap();

    // calling RegisterApiKey because it calls getUpdates (necessary every time a process is restarted)
    let mut pkgs = HashMap::new();
    pkgs.insert(
        Pkg::LLM,
        Address::new(&our.node, ("openai", "command_center", "appattacc.os")),
    );
    pkgs.insert(
        Pkg::STT,
        Address::new(
            &our.node,
            ("speech_to_text", "command_center", "appattacc.os"),
        ),
    );
    pkgs.insert(
        Pkg::Telegram,
        Address::new(&our.node, ("tg", "command_center", "appattacc.os")),
    );

    match &state.clone() {
        Some(state) => {
            if let Some(telegram_key) = &state.config.telegram_key {
                let init = TgInitialize {
                    token: telegram_key.clone(),
                    params: None,
                };
                let req = serde_json::to_vec(&TgRequest::RegisterApiKey(init));
                let _ = Request::new()
                    .target(pkgs.get(&Pkg::Telegram).unwrap())
                    .body(req.unwrap())
                    .send_and_await_response(5);
            }
        }
        None => {}
    }

    let _ = create_drive(our.package_id(), "files", Some(5));

    loop {
        match handle_message(&our, &mut state, &pkgs) {
            Ok(_) => {}
            Err(e) => println!("Error: {:?}", e),
        }
    }
}
