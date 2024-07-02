import { useEffect } from 'react';
import TreeNode from '../TreeNode'
import { Tree } from 'react-arborist';

function Notes({ searchNotes, notesBackupProvider, notesBackedUpAt, lastBackupSize, treeData }) {

    const backupRequest = async () => {
        try {
            const response = await fetch('/main:command_center:appattacc.os/backup', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({
                    our: "123",
                })
            });
            const resp = await response.json();
            console.log("response:", resp);
        } catch (error) {
            console.error("error on backup:", error);
        }
    }


    return (
        <div id="Notes" className="tabcontent">
            <div className="notesBackupStatus">
                <div id="backupStatus" className="flex-col-center">
                    <span id="lastBackupTime">Last backup:</span>
                    <span id="notesBackupProvider">Provider: {notesBackupProvider || "Unknown"}</span>
                    <span id="lastBackupTime">Time: {notesBackedUpAt ? notesBackedUpAt.toLocaleString() : "Unknown"}</span>
                    <span id="lastBackupSize">Size: {lastBackupSize || "Unknown"}</span>
                </div>
            </div>
            <h1 className="mb-2 flex-col-center">Notes</h1>
            <div className="flex-center gap-2">
                <input type="text" id="notesSearch" placeholder="Search Notes" />
            </div>
            <div className="parent-container flex-col-center">
                <button onClick={() => searchNotes()}>Search</button>
                <div className="flex-col-center">
                    <span id="notesResult"></span>
                </div>
            </div>
            <div className="notes-tree" style={{ height: "400px", width: "300px" }}>
                <Tree
                    key={JSON.stringify(treeData)}
                    initialData={treeData}
                    openByDefault={true}
                    width={300}
                    height={400}
                    indent={24}
                    rowHeight={36}
                    overscanCount={1}
                >
                    {TreeNode}
                </Tree>
            </div>
        </div>
    );
}

export default Notes;