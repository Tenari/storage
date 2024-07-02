function ImportNotes({ importNotes }) {
    return (
        <div id="Import Notes" className="tabcontent">
            <h1 className="mb-2 flex-col-center">Import Notes</h1>
            <div className="parent-container flex-col-center">
                <input
                    type="file"
                    id="folderInput"
                    onChange={(e) => importNotes(e)}
                    webkitdirectory="true"
                    multiple
                    style={{ display: 'none' }}
                />
                <label htmlFor="folderInput" className="button">Choose Files</label>
                <div className="flex-col-center">
                    <span id="importNotesResult"></span>
                </div>
            </div>
        </div>
    );
}

export default ImportNotes;