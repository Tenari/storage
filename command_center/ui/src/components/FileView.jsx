import MDEditor from '@uiw/react-md-editor';
import { useState } from 'react';
import '../App.css';

function FileView(note) {
    const [content, setContent] = useState('');
    return (
        <div className="h-screen w-screen overflow-hidden flex flex-col">
            <h2 className="p-4 flex-shrink-0">Viewing file</h2>
            <MDEditor
                value={note.note}
                onChange={setContent}
                className="flex-grow"
                preview="edit"
            />
        </div>
    );
}

export default FileView;