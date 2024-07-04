# Command Center

### Quick Start

Boot up 2 real nodes. Real ones are preferable because we are working with file storage which is easier to persist this way.

```bash
./binary/kinode home
./binary/kinode home2
```

```bash
cd command_center/command_center
kit b && kit s && kit s -p 8081
```
- due to a current bug in kit, you can use `kit b <<< y && kit s && kit s -p 8081` instead, if you don't want to manually press 'y' every time

```bash
cd command_center/command_center/ui
npm i
npm run dev
```

### Working Functionality

In the UI you should be able to use the following tabs:
- Config - set up tg bot
- Data Center - see tg chat in real time
- Import Notes - import notes via ui
- Notes - check backup status, search notes, view directory structure, and view notes in markdown
- Provided backups - see backups which you are providing for other nodes

Backup functionality is a work in progress, so it may not work correctly.

### Dev Setup for Backups

`node1.os` at `home` will be backing up their notes to `node2.os` at `home2`.

Import notes via ui on `node1.os`, they should show up here:
```bash 
cd home/vfs/command_center:appattacc.os/files
ls
```

Throughout the rest of the tutorial, replace `node1.os` and `node2.os` with the node ids of your nodes.

#### Backing Up

To back them up, in `node1.os` terminal, run:
```
m node1.os@main:command_center:appattacc.os '{"BackupRequest": {"node_id": "node2.os", "size": 0, "password_hash": "somehash"}}'
```

In `node2.os` `home2`, you should find a folder called `node1.os`:
``` bash
cd home2/vfs/command_center:appattacc.os/encrypted_storage/
ls
```

Inside that folder should be a bunch of encrypted files
```bash
cd node1.os
ls
```

#### Retrieving Backup

To retrieve the backup to `node1.os`, in `node1.os` terminal, run:
```bash
m node1.os@main:command_center:appattacc.os '{"BackupRetrieve": {"node_id": "node2.os"}}'
```

In `node1.os` `home`, you should find a folder called `retrieved_encrypted_backup` containing the same encrypted files which are backed up to `node2.os`:
```bash
cd home/vfs/command_center:appattacc.os/retrieved_encrypted_backup
ls
```

#### Decrypting Retrieved Backup

Warning: this will overwrite any changes to the notes you made in the `files` directory in the meantime.

To decrypt this data, in `node1.os` terminal, run:
```bash
m node1.os@main:command_center:appattacc.os '{"Decrypt": {"password_hash": "somehash"}}'
```

In `node1.os` `home`, look at `files`, they should contain the decrypted data.
```bash
cd home/vfs/command_center:appattacc.os/files
ls
```

### TODO Functionality
- the commands shown above need to be connected to the ui
- the ui also needs to correctly surface backup status 
    - for client node: when was the last time data was backed up
    - for server node: all nodes which are backing up data to this node, and corresponding statuses
- a setting which allows a node to say: "I provide backups" or "I do not provide backups". Additionally, whitelisting and blacklisting by arbitrary criteria can be done.
- editing notes via markdown and propagating the changes to backend, then to backup
- vector search your notes
