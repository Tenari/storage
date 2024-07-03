use base64::{engine::general_purpose, Engine as _};
use std::path::{self, Path};

use kinode_process_lib::{
    await_message, call_init, get_blob, println,
    vfs::{
        create_file, open_dir, open_file, DirEntry, Directory, FileType, SeekFrom, VfsAction,
        VfsRequest,
    },
    Address, Message, Request,
};

use files_lib::encryption::{encrypt_data, CHUNK_SIZE};
use files_lib::read_nested_dir_light;
use files_lib::structs::WorkerRequest;

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v0",
});

fn handle_message(
    _our: &Address,
    receive_chunks_to_dir: &mut String,
    _size: &mut Option<u64>,
) -> anyhow::Result<bool> {
    let message = await_message()?;

    match message {
        Message::Request { ref body, .. } => {
            let request = serde_json::from_slice::<WorkerRequest>(body)?;
            match request {
                WorkerRequest::InitializeSendWorker {
                    target_worker,
                    password_hash,
                    sending_from_dir
                } => {
                    println!("command_center worker: got initialize request");

                    // initialize command from main process,
                    // sets up worker, matches on if it's a sender or receiver.
                    // if target_worker = None, we are receiver, else sender.
                    // send data to target worker
                    println!("password_hash: {:?}", password_hash);
                    println!("sending_from_dir: {}", sending_from_dir);
                    let dir_entry = DirEntry {
                        path: sending_from_dir,
                        file_type: FileType::Directory
                    };

                    // outputs map path contents, a flattened version of the nested dir
                    let dir = read_nested_dir_light(dir_entry)?;

                    // send each file in the folder to the server
                    for path in dir.keys() {
                        // open/create empty file
                        let mut active_file = open_file(path, false, Some(5))?;

                        // we have a target, chunk the data, and send it.
                        let size = active_file.metadata()?.len;
                        // println!("path: {}", path);

                        let _pos = active_file.seek(SeekFrom::Start(0))?;

                        let file_name =
                        // encrypts file name
                        if let Some(pw_hash) = password_hash.clone() {
                            // path: e.g. command_center:appattacc.os/files/Obsidian Vault/file.md
                            // file_name in request:
                            // GAXPVM7gDutxI3DnsFfhYk5H8vsuYPR1HIXLjJIpFcp4Ip_iXhl7u3voPX_uerfadAldI3PAKVYr0TpPk7qTndv3adGSGWMp9GLUuxPdOLUt84zyETiFgdm2kyYA0pihtLlOiu_E3A==
                            let prefix = "command_center:appattacc.os/files/";
                            if path.starts_with(prefix) {
                                let rest_of_path = &path[prefix.len()..];
                                let encrypted_vec = &encrypt_data(
                                    rest_of_path.as_bytes(),
                                    pw_hash.as_str(),
                                );
                                let rest_of_path =
                                    general_purpose::URL_SAFE.encode(&encrypted_vec);
                                rest_of_path
                            } else {
                                return Err(anyhow::anyhow!(
                                    "Path does not start with the expected prefix"
                                ));
                            }
                        } else 
                        // doesnt encrypt file name
                        {
                            // path: e.g. command_center:appattacc.os/encrypted_storage/node-name.os/GAXPVM7gDutxI3DnsFfhYk5H8vsuYPR1HIXLjJIpFcp4Ip_iXhl7u3voPX_uerfadAldI3PAKVYr0TpPk7qTndv3adGSGWMp9GLUuxPdOLUt84zyETiFgdm2kyYA0pihtLlOiu_E3A.md
                            // file_name in request:
                            // GAXPVM7gDutxI3DnsFfhYk5H8vsuYPR1HIXLjJIpFcp4Ip_iXhl7u3voPX_uerfadAldI3PAKVYr0TpPk7qTndv3adGSGWMp9GLUuxPdOLUt84zyETiFgdm2kyYA0pihtLlOiu_E3A==
                            let path = Path::new(path);
                            path
                                .file_name()
                                .unwrap_or_default()
                                .to_str()
                                .unwrap_or_default()
                                .to_string()
                        };

                            
                        // chunking and sending
                        let num_chunks = if size != 0 {
                            (size as f64 / CHUNK_SIZE as f64).ceil() as u64
                        } else {
                            1
                        };

                        for i in 0..num_chunks {
                            let offset = i * CHUNK_SIZE;
                            let length = CHUNK_SIZE.min(size - offset); // size=file size
                            let mut buffer = vec![0; length as usize];
                            let _pos = active_file.seek(SeekFrom::Current(0))?;
                            active_file.read_at(&mut buffer)?;

                            if let Some(pw_hash) = password_hash.clone() {
                                buffer = encrypt_data(&buffer, pw_hash.as_str());
                            }

                            Request::new()
                                .body(serde_json::to_vec(&WorkerRequest::Chunk {
                                    file_name: file_name.clone(),
                                    done: false,
                                })?)
                                .target(target_worker.clone())
                                .blob_bytes(buffer.clone())
                                .send()?;
                        }
                    }
                    println!("worker: sent everything");
                    Request::new()
                        .body(serde_json::to_vec(&WorkerRequest::Chunk {
                            file_name: "".to_string(),
                            done: true,
                        })?)
                        .target(target_worker.clone())
                        .send()?;

                    return Ok(true);
                }
                WorkerRequest::InitializeReceiveWorker {
                    receive_to_dir
                } => {
                    // start receiving data
                    // we receive only the name of the overfolder (i.e. Obsidian Vault)
                    let full_path = receive_to_dir;
                    *receive_chunks_to_dir = full_path.clone();

                    let request: VfsRequest = VfsRequest {
                        path: full_path.to_string(),
                        action: VfsAction::RemoveDirAll,
                    };
                    let _message = Request::new()
                        .target(("our", "vfs", "distro", "sys"))
                        .body(serde_json::to_vec(&request)?)
                        .send_and_await_response(5)?;

                    println!("starting to receive data for dir: {}", full_path);

                    // maybe this is unnecessary in both cases (whether retrieving backup or backing up)?
                    let request: VfsRequest = VfsRequest {
                        path: full_path.to_string(),
                        action: VfsAction::CreateDirAll,
                    };

                    let _message = Request::new()
                        .target(("our", "vfs", "distro", "sys"))
                        .body(serde_json::to_vec(&request)?)
                        .send_and_await_response(5)?;
                }
                // someone sending a chunk to us!
                WorkerRequest::Chunk {
                    file_name,
                    done,
                } => {
                    if done == true {
                        return Ok(true);
                    }
                    let blob = get_blob();

                    let path_to_dir = &receive_chunks_to_dir[1..]; // just skipping the initial '/'

                    let file_path = format!("/{}/{}", path_to_dir, &file_name);
                    let _dir = open_dir(&format!("/{}", path_to_dir), false, Some(5))?;

                    let bytes = match blob {
                        Some(blob) => blob.bytes,
                        None => {
                            return Err(anyhow::anyhow!("command_center: receive error: no blob"));
                        }
                    };

                    // manually creating file if doesnt exist, since open_file(create:true) has an issue
                    let dir = open_dir(&path_to_dir, false, None)?;

                    let entries = dir.read()?;

                    if entries.contains(&DirEntry {
                        path: file_path[1..].to_string(),
                        file_type: FileType::File,
                    }) {
                    } else {
                        let _file = create_file(&file_path, Some(5))?;
                    }

                    let mut file = open_file(&file_path, false, Some(5))?;
                    file.append(&bytes)?;
                }
            }
        }
        _ => {
            println!("command_center worker: got something else than request...");
        }
    }
    Ok(false)
}

call_init!(init);
fn init(our: Address) {
    println!("command_center worker: begin");
    let start = std::time::Instant::now();

    let mut receive_chunks_to_dir = String::new();

    // TODO size should be a hashmap of sizes for each file(?)
    let mut size: Option<u64> = None;

    loop {
        match handle_message(
            &our,
            &mut receive_chunks_to_dir,
            &mut size,
        ) {
            Ok(exit) => {
                if exit {
                    println!(
                        "command_center worker: done: exiting, took {:?}",
                        start.elapsed()
                    );
                    break;
                }
            }
            Err(e) => {
                println!("command_center: worker error: {:?}", e);
            }
        };
    }
}
