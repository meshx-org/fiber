wit_bindgen::generate!({ generate_all });

use crate::exports::meshx::data::schema::Guest;
use crate::wasi::keyvalue::store::*;
use exports::meshx::data::schema;
use exports::meshx::data::schema::{Command, CommandError};
use crate::meshx::data::types;
use murmur3::murmur3_32;
use serde::{Deserialize, Serialize};
use std::io::Cursor;

struct SchemaManager;

#[derive(Serialize, Deserialize, Debug)]
struct Field {
    name: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Table {
    name: String,
    fields: Vec<Field>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Schema {
    tables: Vec<Table>,
}

fn mutate_schema(schema: &mut Schema, commands: &Vec<Command>) {
    for mutation in commands.iter() {
        match mutation {
            Command::DeclareField(cmd) => {
                /*let table = match schema.tables.iter().find(|t| t.name == tag_name) {
                    Some(existing) => existing,
                    None => {
                        schema.tables.push(Table {
                            name: cmd.table_name,
                            fields: vec![],
                        });
                        schema.tables.last().unwrap()
                    }
                };*/
                todo!()
            }
            Command::DeclareTable(cmd) => todo!(),
        }
    }
}

impl Guest for SchemaManager {
    fn run_commands(realm: String, commands: Vec<Command>) -> Result<(), Vec<CommandError>> {
        match open("schema") {
            Ok(bucket) => {
                let data = bucket.get(&format!("next_{}", realm)).unwrap();

                match data {
                    Some(bytes) => {
                        let string = String::from_utf8(bytes).unwrap();
                        let mut schema = serde_json::from_str::<Schema>(&string).unwrap();

                        mutate_schema(&mut schema, &commands);

                        bucket.set(
                            &format!("next_{}", realm),
                            serde_json::to_string(&schema).unwrap().as_bytes(),
                        );
                    }
                    None => {
                        let mut schema = Schema { tables: vec![] };
                        mutate_schema(&mut schema, &commands);
                    }
                }

                Ok(())
            }
            Err(_) => todo!(),
        }
    }

    fn get_schema(realm: Option<String>, draft: bool) -> schema::Schema {
        /*match open("schema") {
            Ok(bucket) => {
                let key_response = bucket.list_keys(None).unwrap();
                key_response.keys;
            }
            Err(_) => todo!(),
        }*/

        let table_hash = murmur3_32(&mut Cursor::new(b"users"), 0).unwrap();
        let id_hash = murmur3_32(&mut Cursor::new(b"id"), 0).unwrap();

        schema::Schema {
            tables: vec![(
                format!("{:08x}", table_hash),
                schema::Table {
                    fields: vec![(
                        format!("{:08x}", id_hash),
                        schema::Field {
                            name: None,
                            physical_name: "id".to_owned(),
                            meta: vec![],
                            data_type: types::FieldType::Text,
                        },
                    )],
                    name: None,
                    physical_name: "users".to_owned(),
                    meta: vec![],
                },
            )],
        }
    }
}

export!(SchemaManager);
