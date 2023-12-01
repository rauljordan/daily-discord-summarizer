-- Create the 'daily_digests' table
CREATE TABLE daily_digests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    text TEXT NOT NULL,
    timestamp DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Create the 'summaries' table with a foreign key reference to 'daily_digests'
CREATE TABLE summaries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    daily_digest_id INTEGER,
    text TEXT NOT NULL,
    timestamp DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (daily_digest_id) REFERENCES daily_digests(id)
);