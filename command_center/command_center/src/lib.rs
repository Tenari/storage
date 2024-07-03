use base64::{engine::general_purpose, Engine as _};
use std::collections::HashMap;
use std::path::Path;

use kinode_process_lib::vfs::{
    create_drive, create_file, open_dir, open_file, DirEntry, FileType, SeekFrom, VfsAction,
    VfsRequest,
};
use kinode_process_lib::{
    await_message, call_init, get_blob, http, our_capabilities, println, spawn, Address, Message,
    OnExit, Request, Response,
};

use llm_interface::openai::*;
use stt_interface::*;
use telegram_interface::*;

mod structs;
use structs::*;

mod tg_api;

use files_lib::encryption::{decrypt_data, ENCRYPTED_CHUNK_SIZE};
use files_lib::structs::{
    BackupRequestResponse, ClientRequest, ServerResponse, UiRequest, WorkerRequest,
    WorkerRequestType,
};
use files_lib::{import_notes, read_nested_dir_light};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v0",
});

/////////////////////////////////////////////////
// functions that fulfill HTTP requests from UI

fn fetch_backup_data(state: &mut State) -> anyhow::Result<()> {
    let backup_data = serde_json::to_vec(&serde_json::json!({
        "backups_time_map": state.backup_info.backups_time_map,
        "notes_last_backed_up_at": state.backup_info.notes_last_backed_up_at,
        "notes_backup_provider": state.backup_info.notes_backup_provider,
    }))?;
    http::send_response(
        http::StatusCode::OK,
        Some(HashMap::from([(
            "Content-Type".to_string(),
            "application/json".to_string(),
        )])),
        backup_data,
    );
    Ok(())
}

fn fetch_status(state: &mut State) -> anyhow::Result<()> {
    let config = &state.api_keys;
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

fn fetch_notes(our_files_path: &String) -> anyhow::Result<()> {
    let dir_entry: DirEntry = DirEntry {
        path: our_files_path.to_string(),
        file_type: FileType::Directory,
    };

    let notes = files_lib::read_nested_dir(dir_entry)?;

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

fn submit_api_keys(
    state: &mut State,
    pkgs: &HashMap<Pkg, Address>,
    body_bytes: &[u8],
) -> anyhow::Result<()> {
    let api_keys = serde_json::from_slice::<ApiKeys>(body_bytes)?;
    println!("Modifying api_keys to {:?}", api_keys);
    state.api_keys = api_keys;

    for (pkg, addr) in pkgs.iter() {
        println!("submit_api_keys: matching pkg: {:?}", pkg);
        match pkg {
            Pkg::LLM => {
                if let Some(openai_key) = &state.api_keys.openai_key {
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
                if let Some(groq_key) = &state.api_keys.groq_key {
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
                if let Some(openai_key) = &state.api_keys.openai_key {
                    let req = serde_json::to_vec(&STTRequest::RegisterApiKey(openai_key.clone()))?;
                    let _ = Request::new()
                        .target(addr.clone())
                        .body(req)
                        .send_and_await_response(5)??;
                }
            }
            Pkg::Telegram => {
                if let Some(telegram_key) = &state.api_keys.telegram_key {
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
    Ok(())
}

fn import_notes_and_respond(body_bytes: &[u8]) -> anyhow::Result<()> {
    if import_notes(&body_bytes).is_ok() {
        http::send_response(
            http::StatusCode::OK,
            Some(HashMap::from([(
                "Content-Type".to_string(),
                "application/json".to_string(),
            )])),
            b"{\"message\": \"success\"}".to_vec(),
        );
        Ok(())
    } else {
        Err(anyhow::anyhow!("Failed to import notes"))
    }
}

/////////////////////////////////////////////////

fn initialize_worker(our: Address) -> anyhow::Result<Address> {
    let our_worker = spawn(
        None,
        &format!("{}/pkg/worker.wasm", our.package_id()),
        OnExit::None,
        our_capabilities(),
        vec![],
        false,
    )?;
    Ok(Address {
        node: our.node.clone(),
        process: our_worker,
    })
}

fn handle_ui_backup_request(
    our: &Address,
    state: &mut State,
    paths: &HashMap<&str, String>,
    message: &Message,
) -> anyhow::Result<()> {
    match &message {
        Message::Request { body, .. } => {
            let deserialized = serde_json::from_slice::<UiRequest>(body)?;

            // abstract into separate fn, maybe handle_ui_backups_message
            match deserialized {
                // making backup retrieval request to server
                UiRequest::BackupRetrieve { node_id } => {
                    let our_worker_address = initialize_worker(our.clone())?;

                    let backup_retrieve = serde_json::to_vec(&ClientRequest::BackupRetrieve {
                        worker_address: our_worker_address.clone(),
                    })?;
                    let _retrieve_backup = Request::to(Address::new(
                        node_id.clone(),
                        ("main", "command_center", "appattacc.os"),
                    ))
                    .expects_response(5)
                    .body(backup_retrieve)
                    .send();

                    let _worker_request: Message = Request::new()
                        .body(serde_json::to_vec(&WorkerRequest::Initialize {
                            request_type: WorkerRequestType::RetrievingBackup,
                            uploader_node: Some(our.node.clone()),
                            target_worker: None,
                            password_hash: state.backup_info.data_password_hash.clone(),
                        })?)
                        .target(&our_worker_address)
                        .send_and_await_response(5)??;
                }
                // making backup request to server
                UiRequest::BackupRequest {
                    node_id,
                    password_hash,
                    ..
                } => {
                    state.backup_info.data_password_hash = Some(password_hash.clone());
                    state.save();

                    let backup_request =
                        serde_json::to_vec(&ClientRequest::BackupRequest { size: 0 })?;
                    let _ = Request::to(Address::new(
                        node_id,
                        ("main", "command_center", "appattacc.os"),
                    ))
                    .expects_response(5)
                    .body(backup_request)
                    .send();
                }
                // decrypt retrieved backup
                UiRequest::Decrypt { password_hash, .. } => {
                    // /command_center:appattacc.os/retrieved_encrypted_backup
                    let dir_entry: DirEntry = DirEntry {
                        path: paths.get("retrieved_encrypted_backup_path").unwrap().clone(),
                        file_type: FileType::Directory,
                    };

                    let request: VfsRequest = VfsRequest {
                        path: paths.get("temp_files_path").unwrap().clone(),
                        action: VfsAction::RemoveDirAll,
                    };
                    let _message = Request::new()
                        .target(("our", "vfs", "distro", "sys"))
                        .body(serde_json::to_vec(&request)?)
                        .send_and_await_response(5)?;

                    let request: VfsRequest = VfsRequest {
                        path: paths.get("temp_files_path").unwrap().clone(),
                        action: VfsAction::CreateDirAll,
                    };
                    let _message = Request::new()
                        .target(("our", "vfs", "distro", "sys"))
                        .body(serde_json::to_vec(&request)?)
                        .send_and_await_response(5)?;

                    let dir = read_nested_dir_light(dir_entry)?;
                    // decrypt each file
                    for path in dir.keys() {
                        // open/create empty file
                        let mut active_file = open_file(path, false, Some(5))?;

                        // chunk the data
                        let size = active_file.metadata()?.len;

                        let mut file_name = String::new();
                        let _pos = active_file.seek(SeekFrom::Start(0))?;

                        // path: e.g. command_center:appattacc.os/retrieved_encrypted_backup/GAXPVM7gDutxI3DnsFfhYk5H8vsuYPR1HIXLjJIpFcp4Ip_iXhl7u3voPX_uerfadAldI3PAKVYr0TpPk7qTndv3adGSGWMp9GLUuxPdOLUt84zyETiFgdm2kyYA0pihtLlOiu_E3A
                        println!("path: {}", path);
                        let path = Path::new(path);
                        file_name = path
                            .file_name()
                            .unwrap_or_default()
                            .to_str()
                            .unwrap_or_default()
                            .to_string();

                        println!("file name pre decryption: {}", file_name);
                        // file name decryption
                        let decoded_vec = general_purpose::URL_SAFE.decode(&file_name)?;
                        let decrypted_vec = match decrypt_data(&decoded_vec, password_hash.as_str())
                        {
                            Ok(vec) => vec,
                            Err(e) => {
                                println!("couldn't decrypt file name");
                                return Err(anyhow::anyhow!(e));
                            }
                        };
                        let decrypted_path = String::from_utf8(decrypted_vec).map_err(|e| {
                            anyhow::anyhow!("Failed to convert bytes to string: {}", e)
                        })?;
                        let file_path = format!(
                            "{}/{}",
                            paths.get("temp_files_path").unwrap().clone(),
                            decrypted_path
                        );
                        println!("file_path: {}", file_path);
                        let parent_path = Path::new(&file_path)
                            .parent()
                            .and_then(|p| p.to_str())
                            .unwrap_or("")
                            .to_string();
                        println!("parent_path: {}", parent_path);
                        let request = VfsRequest {
                            path: format!("/{}", parent_path).to_string(),
                            action: VfsAction::CreateDirAll,
                        };
                        let _message = Request::new()
                            .target(("our", "vfs", "distro", "sys"))
                            .body(serde_json::to_vec(&request)?)
                            .send_and_await_response(5)?;
                        let _dir = open_dir(&parent_path, false, Some(5))?;
                        println!("parent path created: {}", parent_path);

                        // chunking and decrypting
                        // have to deal with encryption change the length of buffer
                        // hence offset needs to be accumulated and length of each chunk sent can change
                        let num_chunks = (size as f64 / ENCRYPTED_CHUNK_SIZE as f64).ceil() as u64;

                        //     println!("here?3");

                        for i in 0..num_chunks {
                            let offset = i * ENCRYPTED_CHUNK_SIZE;
                            let length = ENCRYPTED_CHUNK_SIZE.min(size - offset); // size=file size
                            let mut buffer = vec![0; length as usize];
                            let _pos = active_file.seek(SeekFrom::Current(0))?;
                            active_file.read_at(&mut buffer)?;
                            println!("here?4");

                            // decrypting data
                            let decrypted_bytes =
                                match decrypt_data(&buffer, password_hash.as_str()) {
                                    Ok(vec) => vec,
                                    Err(_e) => {
                                        println!("couldn't decrypt file data");
                                        return Err(anyhow::anyhow!("couldn't decrypt file data"));
                                    }
                                };

                            let dir = open_dir(&parent_path, false, None)?;
                            println!("here2.5");

                            let entries = dir.read()?;
                            println!("here2.6");

                            if entries.contains(&DirEntry {
                                path: file_path[1..].to_string(),
                                file_type: FileType::File,
                            }) {
                            } else {
                                println!("here2.7");

                                let _file = create_file(&file_path, Some(5))?;
                            }

                            println!("here3");

                            let mut file = open_file(&file_path, false, Some(5))?;
                            file.append(&decrypted_bytes)?;
                        }
                    }

                    // remove after all decryption is successful,
                    // remove files folder and rename files_temp to files
                    let request: VfsRequest = VfsRequest {
                        path: paths.get("our_files_path").unwrap().clone(),
                        action: VfsAction::RemoveDirAll,
                    };
                    let _message = Request::new()
                        .target(("our", "vfs", "distro", "sys"))
                        .body(serde_json::to_vec(&request)?)
                        .send_and_await_response(5)?;

                    let request: VfsRequest = VfsRequest {
                        path: paths.get("temp_files_path").unwrap().clone(),
                        action: VfsAction::Rename {
                            new_path: paths.get("our_files_path").unwrap().clone().to_string(),
                        },
                    };
                    let _message = Request::new()
                        .target(("our", "vfs", "distro", "sys"))
                        .body(serde_json::to_vec(&request)?)
                        .send_and_await_response(5)?;

                    let _ = create_drive(our.package_id(), "files_temp", Some(5));
                }
            }
            Ok(())
        }
        _ => return Ok(()),
    }
}

fn handle_backup_message(
    our: &Address,
    state: &mut State,
    message: &Message,
) -> anyhow::Result<()> {
    match &message {
        Message::Request { body, .. } => {
            let deserialized: ClientRequest = serde_json::from_slice::<ClientRequest>(body)?;
            match deserialized {
                // receiving backup retrieval request from client
                ClientRequest::BackupRetrieve { worker_address } => {
                    let our_worker_address = initialize_worker(our.clone())?;
                    let _worker_request = Request::new()
                        .body(serde_json::to_vec(&WorkerRequest::Initialize {
                            request_type: WorkerRequestType::RetrievingBackup,
                            uploader_node: None,
                            target_worker: Some(worker_address),
                            password_hash: None,
                        })?)
                        .target(&our_worker_address)
                        .send()?;

                    let backup_response: Vec<u8> =
                        serde_json::to_vec(&ServerResponse::BackupRetrieveResponse(
                            state
                                .backup_info
                                .backups_time_map
                                .get(&message.source().node)
                                .copied(),
                        ))?;
                    let _resp: Result<(), anyhow::Error> =
                        Response::new().body(backup_response).send();
                }
                // receiving backup request from client
                ClientRequest::BackupRequest { .. } => {
                    // TODO: add criterion here
                    // whether we want to provide backup or not

                    state
                        .backup_info
                        .backups_time_map
                        .insert(message.source().node.to_string(), chrono::Utc::now());
                    state.save();

                    let our_worker_address = initialize_worker(our.clone())?;

                    let backup_response: Vec<u8> = serde_json::to_vec(
                        &ServerResponse::BackupRequestResponse(BackupRequestResponse::Confirm {
                            worker_address: our_worker_address.clone(),
                        }),
                    )?;
                    let _resp: Result<(), anyhow::Error> =
                        Response::new().body(backup_response).send();

                    let _worker_request = Request::new()
                        .body(serde_json::to_vec(&WorkerRequest::Initialize {
                            request_type: WorkerRequestType::BackingUp,
                            uploader_node: Some(message.source().node.clone()),
                            target_worker: None,
                            password_hash: None,
                        })?)
                        .target(&our_worker_address)
                        .send_and_await_response(5)??;
                }
            }
        }
        // receiving backup response from server
        Message::Response { body, .. } => {
            let deserialized: ServerResponse = serde_json::from_slice::<ServerResponse>(body)?;
            match deserialized {
                ServerResponse::BackupRetrieveResponse(datetime) => {
                    state.backup_info.notes_last_backed_up_at = datetime;
                    state.save()
                }
                ServerResponse::BackupRequestResponse(backup_response) => match backup_response {
                    BackupRequestResponse::Confirm { worker_address } => {
                        println!(
                            "received Confirm backup_response from {:?}",
                            message.source().node,
                        );

                        let our_worker_address = initialize_worker(our.clone())?;
                        let _worker_request = Request::new()
                            .body(serde_json::to_vec(&WorkerRequest::Initialize {
                                request_type: WorkerRequestType::BackingUp,
                                uploader_node: None,
                                target_worker: Some(worker_address),
                                password_hash: state.backup_info.data_password_hash.clone(),
                            })?)
                            .target(&our_worker_address)
                            .send()?;

                        state.backup_info.notes_last_backed_up_at = Some(chrono::Utc::now());
                        state.backup_info.notes_backup_provider =
                            Some(message.source().node.clone());
                        state.backup_info.data_password_hash = None;
                        state.save();
                    }
                    BackupRequestResponse::Decline { .. } => {
                        println!(
                            "received Decline backup_response from {:?}",
                            message.source().node,
                        );
                    }
                },
            }
        }
    }
    return Ok(());
}

fn handle_http_request(
    state: &mut State,
    pkgs: &HashMap<Pkg, Address>,
    paths: &HashMap<&str, String>,
    body: &[u8],
) -> anyhow::Result<()> {
    let http_request = http::HttpServerRequest::from_bytes(body)?
        .request()
        .ok_or_else(|| anyhow::anyhow!("Failed to parse http request"))?;
    let path = http_request.path()?;
    let bytes = get_blob()
        .ok_or_else(|| anyhow::anyhow!("Failed to get blob"))?
        .bytes;
    match path.as_str() {
        "/status" => fetch_status(state),
        "/fetch_backup_data" => fetch_backup_data(state),
        "/submit_api_keys" => submit_api_keys(state, pkgs, &bytes),
        "/notes" => fetch_notes(paths.get("our_files_path").unwrap()),
        "/import_notes" => import_notes_and_respond(&bytes),
        "/backup_request" => {
            // WIP
            println!("got /backup_request");
            let deserialized: Result<serde_json::Value, _> = serde_json::from_slice(&bytes);
            match deserialized {
                Ok(value) => {
                    println!("Deserialized backup request: {:?}", value);
                    Ok(())
                }
                Err(e) => {
                    println!("Error deserializing backup request: {:?}", e);
                    println!("Received bytes: {:?}", String::from_utf8_lossy(&bytes));
                    Err(anyhow::anyhow!(
                        "Failed to deserialize backup request: {}",
                        e
                    ))
                }
            }
        }
        _ => Ok(()),
    }
}

fn handle_http_message(
    state: &mut State,
    pkgs: &HashMap<Pkg, Address>,
    paths: &HashMap<&str, String>,
    message: &Message,
) -> anyhow::Result<()> {
    match message {
        Message::Request { ref body, .. } => handle_http_request(state, pkgs, paths, body),
        Message::Response { .. } => Ok(()),
    }
}

fn handle_message(
    our: &Address,
    state: &mut State,
    pkgs: &HashMap<Pkg, Address>,
    paths: &HashMap<&str, String>,
) -> anyhow::Result<()> {
    let message = await_message()?;

    if message.source().node != our.node {
        handle_backup_message(our, state, &message)?;
    }

    match message.source().process.to_string().as_str() {
        "http_server:distro:sys" | "http_client:distro:sys" => {
            handle_http_message(state, pkgs, paths, &message)
        }
        // helper for debugging. remove for prod.
        // it takes inputs from the teriminal
        _ => handle_ui_backup_request(our, state, paths, &message),
    }
}

const ICON: &str = include_str!("icon");
call_init!(init);
fn init(our: Address) {
    let _ = http::serve_ui(
        &our,
        "ui",
        true,
        false,
        vec![
            "/",
            "/submit_api_keys",
            "/status",
            "/notes",
            "/import_notes",
            "/backup_request",
            "/fetch_backup_data",
        ],
    );

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

    let mut state = State::fetch()
        .unwrap_or_else(|| State::new(&our, ApiKeys::default(), BackupInfo::default()));

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

    // calling RegisterApiKey because it calls getUpdates (necessary every time a process is restarted)
    if let Some(telegram_key) = &state.api_keys.telegram_key {
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

    let temp_files_path = create_drive(our.package_id(), "files_temp", Some(5)).unwrap();
    let our_files_path = create_drive(our.package_id(), "files", Some(5)).unwrap();
    let encrypted_storage_path =
        create_drive(our.package_id(), "encrypted_storage", Some(5)).unwrap();
    let retrieved_encrypted_backup_path =
        create_drive(our.package_id(), "retrieved_encrypted_backup", Some(5)).unwrap();
    let mut paths = HashMap::new();
    paths.insert("our_files_path", our_files_path);
    paths.insert("temp_files_path", temp_files_path);
    paths.insert(
        "retrieved_encrypted_backup_path",
        retrieved_encrypted_backup_path,
    );
    paths.insert("encrypted_storage_path", encrypted_storage_path);

    loop {
        match handle_message(&our, &mut state, &pkgs, &paths) {
            Ok(_) => {}
            Err(e) => println!("Error: {:?}", e),
        }
    }
}
