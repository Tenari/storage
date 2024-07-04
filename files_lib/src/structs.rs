use chrono::{DateTime, Utc};
use kinode_process_lib::{Address, NodeId};
use serde::{Deserialize, Serialize};

// ui -> client node
#[derive(Serialize, Deserialize, Debug)]
pub enum UiRequest {
    BackupRequest {
        node_id: NodeId,
        size: u64,
        password_hash: String,
    },
    BackupRetrieve {
        node_id: NodeId,
    },
    Decrypt {
        password_hash: String,
    },
}

// client node -> server node
#[derive(Serialize, Deserialize, Debug)]
pub enum ClientRequest {
    // telling the server which data size to expect
    BackupRequest { size: u64 },
    BackupRetrieve { worker_address: Address },
}

// server node -> client node
#[derive(Serialize, Deserialize, Debug)]
pub enum ServerResponse {
    BackupRequestResponse(BackupRequestResponse),
    BackupRetrieveResponse(Option<DateTime<Utc>>),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum BackupRequestResponse {
    Confirm { worker_address: Address },
    Decline,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum WorkerRequest {
    InitializeSenderWorker {
        target_worker: Address,
        password_hash: Option<String>, // if has password_hash, encrypts; otherwise, no encryption
        sending_from_dir: String,
    },
    InitializeReceiverWorker {
        receive_to_dir: String,
    },
    Chunk {
        done: bool,
        file_name: String,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum WorkerStatus {
    Done,
}