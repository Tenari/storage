function ProvidedBackups({ backupsTimeMap }) {
    return (
        <div id="Provided Backups" className="tabcontent">
            <h1 className="mb-2 flex-col-center">Provided Backups</h1>
            <div className="parent-container flex-col-center">
                <table className="backup-table">
                    <thead>
                        <tr>
                            <th>Provider</th>
                            <th>Last Backup Time</th>
                        </tr>
                    </thead>
                    <tbody>
                        {Array.from(backupsTimeMap).map(([provider, time]) => (
                            <tr key={provider}>
                                <td>{provider}</td>
                                <td>{time.toLocaleString()}</td>
                            </tr>
                        ))}
                    </tbody>
                </table>
            </div>
        </div>
    );
}

export default ProvidedBackups;

