use rusqlite::Connection;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("DB Error: {0}")]
    DbError(#[from] rusqlite::Error),
}

pub type WordAndDefinition = (String, String);

pub trait Database {
    fn is_populated(&self) -> Result<bool, Error>;
    fn clear(&self) -> Result<(), Error>;
    fn populate_chunk(&self, word_and_definitions: Vec<WordAndDefinition>) -> Result<(), Error>;
}

pub struct SqliteDatabase {
    connection: Connection,
}

impl SqliteDatabase {
    pub fn build() -> Result<Self, Error> {
        Ok(Self {
            connection: create_connection()?,
        })
    }
}

fn db_path() -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "db", "whatchamacallit.db"]
        .iter()
        .collect()
}

fn create_connection() -> Result<Connection, Error> {
    init_vec_extensions()?;

    let connection = Connection::open(db_path())?;
    init_tables(&connection)?;
    init_indices(&connection)?;

    Ok(connection)
}

fn init_vec_extensions() -> Result<(), Error> {
    unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    }
    Ok(())
}

fn init_tables(connection: &Connection) -> Result<(), Error> {
    init_words(connection)?;
    init_word_embeddings(connection)?;
    init_antonyms(connection)?;
    init_defs(connection)
}

fn init_word_embeddings(connection: &Connection) -> Result<(), Error> {
    let sql = "
CREATE VIRTUAL TABLE IF NOT EXISTS word_embeddings USING vec0(
    id INTEGER PRIMARY KEY,
    embedding FLOAT[384]
);";

    connection.execute(sql, [])?;
    Ok(())
}

fn init_words(connection: &Connection) -> Result<(), Error> {
    let sql = "
CREATE TABLE IF NOT EXISTS words (
    id               INTEGER  PRIMARY KEY  AUTOINCREMENT,
    word             TEXT     UNIQUE       NOT NULL
);";

    connection.execute(sql, [])?;
    Ok(())
}

fn init_antonyms(connection: &Connection) -> Result<(), Error> {
    let sql = "
CREATE TABLE IF NOT EXISTS antonyms (
    id               INTEGER  PRIMARY KEY  AUTOINCREMENT,
    left_word_id     INTEGER               NOT NULL,
    right_word_id    INTEGER               NOT NULL
);";

    connection.execute(sql, [])?;
    Ok(())
}

fn init_defs(connection: &Connection) -> Result<(), Error> {
    let sql = "
CREATE TABLE IF NOT EXISTS defs (
    id               INTEGER  PRIMARY KEY  AUTOINCREMENT,
    word_id          INTEGER               NOT NULL,
    text_definition  TEXT     UNIQUE       NOT NULL
);";

    connection.execute(sql, [])?;
    Ok(())
}

fn init_antonym_temp_table(connection: &Connection) -> Result<(), Error> {
    let sql = "
CREATE TEMP TABLE IF NOT EXISTS temp.antonyms (
    id               INTEGER  PRIMARY KEY  AUTOINCREMENT,
    left_word_id     TEXT                  NOT NULL,
    right_word_id    TEXT                  NOT NULL
);";

    connection.execute(sql, [])?;
    Ok(())
}

fn init_drop_antonym_temp_table(connection: &Connection) -> Result<(), Error> {
    let sql = "DROP TABLE IF EXISTS temp.antonyms;";

    connection.execute(sql, [])?;
    Ok(())
}

fn init_indices(connection: &Connection) -> Result<(), Error> {
    init_words_word_index(connection)?;
    init_defs_word_index(connection)?;
    Ok(())
}

fn init_words_word_index(connection: &Connection) -> Result<(), Error> {
    let create_words_word_index = "CREATE INDEX IF NOT EXISTS words_word ON words (word);";
    connection.execute(create_words_word_index, [])?;
    Ok(())
}

fn init_defs_word_index(connection: &Connection) -> Result<(), Error> {
    let create_words_word_index = "CREATE INDEX IF NOT EXISTS defs_word_id ON defs (word_id);";
    connection.execute(create_words_word_index, [])?;
    Ok(())
}

impl Database for SqliteDatabase {
    fn is_populated(&self) -> Result<bool, Error> {
        let pop: u32 = self
            .connection
            .query_row("SELECT COUNT(1) FROM words", [], |row| row.get(0))?;

        Ok(pop > 0)
    }

    fn clear(&self) -> Result<(), Error> {
        let sql = "TRUNCATE TABLE words;  TRUNCATE TABLE word_embeddings;";
        self.connection.execute(sql, [])?;
        Ok(())
    }

    fn populate_chunk(&self, word_and_definitions: Vec<WordAndDefinition>) -> Result<(), Error> {
        let values = word_and_definitions
            .iter()
            .filter_map(|(word, def)| {
                let word = escape_for_sqlite(word);
                let def = escape_for_sqlite(def);

                if word.is_empty() || def.is_empty() {
                    return None;
                }

                Some(format!("('{word}', '{def}')"))
            })
            .collect::<Vec<_>>()
            .join(" ");

        let sql = format!("INSERT INTO words (word, text_definition) VALUES {values};");
        self.connection.execute(&sql, [])?;

        Ok(())
    }
}

fn escape_for_sqlite(text: &str) -> String {
    let result: String = text.chars().filter(|c| !c.is_control()).collect();
    let result = result.replace("'", "''");
    result.trim().to_string()
}
