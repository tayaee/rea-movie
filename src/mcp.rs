use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::assemble::{self, AssembleOptions};
use crate::config::ProjectConfig;
use crate::decompile::{self, DecompileOptions};
use crate::pool;
use crate::render::{self, RenderOptions};
use crate::sync_prompt::compile_prompt_file;
use crate::timeline::{ShotMeta, TimelineManifest};

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

pub fn run_mcp_server() -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out_handle = stdout.lock();

    for line_res in stdin.lock().lines() {
        let line = match line_res {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                let err_res = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {}", e)
                    }
                });
                writeln!(out_handle, "{}", err_res)?;
                out_handle.flush()?;
                continue;
            }
        };

        if let Some(resp) = handle_request(&req) {
            writeln!(out_handle, "{}", serde_json::to_string(&resp)?)?;
            out_handle.flush()?;
        }
    }

    Ok(())
}

fn handle_request(req: &JsonRpcRequest) -> Option<Value> {
    let req_id = req.id.clone().unwrap_or(Value::Null);

    match req.method.as_str() {
        "initialize" => {
            let res = json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "rea-movie",
                        "version": "0.1.0"
                    }
                }
            });
            Some(res)
        }
        "notifications/initialized" | "initialized" => {
            // Notification: no response required
            None
        }
        "ping" => {
            let res = json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {}
            });
            Some(res)
        }
        "tools/list" => {
            let tools = get_tool_definitions();
            let res = json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "tools": tools
                }
            });
            Some(res)
        }
        "tools/call" => {
            let params = req.params.as_ref();
            let tool_name = params
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("");
            let arguments = params
                .and_then(|p| p.get("arguments"))
                .cloned()
                .unwrap_or_else(|| json!({}));

            let (text, is_error) = match execute_tool(tool_name, &arguments) {
                Ok(msg) => (msg, false),
                Err(err) => (format!("Error executing {}: {:#}", tool_name, err), true),
            };

            let res = json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": text
                        }
                    ],
                    "isError": is_error
                }
            });
            Some(res)
        }
        _ => {
            if req.id.is_some() {
                let res = json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "error": {
                        "code": -32601,
                        "message": format!("Method not found: {}", req.method)
                    }
                });
                Some(res)
            } else {
                None
            }
        }
    }
}

fn get_tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "decompile_movie",
            "description": "Reverse-engineer input video (.mp4, .mkv) into shot-by-shot MiniMax-H3 multi-shot project",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": {
                        "type": "string",
                        "description": "Path to input video file"
                    },
                    "output": {
                        "type": "string",
                        "description": "Output project directory"
                    },
                    "range": {
                        "type": "string",
                        "description": "Time range to decompile (format: START-END, e.g. 00:01:00-00:02:30)"
                    },
                    "max_shot_duration": {
                        "type": "number",
                        "description": "Maximum shot duration before split (default: 8.0)"
                    },
                    "force": {
                        "type": "boolean",
                        "description": "Force overwrite existing assets and prompts"
                    },
                    "run": {
                        "type": "boolean",
                        "description": "Automatically run render and assemble after decompile"
                    },
                    "smoke": {
                        "type": "boolean",
                        "description": "Use smoke profile instead of prod"
                    },
                    "offline": {
                        "type": "boolean",
                        "description": "Run in offline mode without MiniMax vision API"
                    }
                },
                "required": ["input"]
            }
        }),
        json!({
            "name": "list_shots",
            "description": "List all shots in the project, their metadata, duration, anchor, and render status (prod/smoke)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "proj_dir": {
                        "type": "string",
                        "description": "Project directory path (default: .)"
                    }
                }
            }
        }),
        json!({
            "name": "inspect_shot",
            "description": "Inspect a specific shot's metadata (shot.toml), Korean director prompt (prompt.ko.md), and H3 prompt (prompt.h3.txt)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "shot_id": {
                        "type": "string",
                        "description": "Shot identifier (e.g. shot_0001 or 0001)"
                    },
                    "proj_dir": {
                        "type": "string",
                        "description": "Project directory path (default: .)"
                    }
                },
                "required": ["shot_id"]
            }
        }),
        json!({
            "name": "update_shot_prompt",
            "description": "Update Korean director prompt (prompt.ko.md), H3 prompt (prompt.h3.txt), or anchor setting in shot.toml",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "shot_id": {
                        "type": "string",
                        "description": "Shot identifier (e.g. shot_0001 or 0001)"
                    },
                    "proj_dir": {
                        "type": "string",
                        "description": "Project directory path (default: .)"
                    },
                    "prompt_ko": {
                        "type": "string",
                        "description": "New content for prompt.ko.md"
                    },
                    "prompt_h3": {
                        "type": "string",
                        "description": "New content for prompt.h3.txt"
                    },
                    "anchor": {
                        "type": "string",
                        "description": "New anchor mode (e.g. first_and_last, first_frame, ref_only, text_only)"
                    },
                    "recompile": {
                        "type": "boolean",
                        "description": "Automatically recompile prompt.ko.md into prompt.h3.txt (default: true if prompt_ko changed)"
                    }
                },
                "required": ["shot_id"]
            }
        }),
        json!({
            "name": "render_shot",
            "description": "Render specific shot, range, or all shots using DGX Spark MiniMax-H3 server pool",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "proj_dir": {
                        "type": "string",
                        "description": "Project directory path (default: .)"
                    },
                    "shot": {
                        "type": "string",
                        "description": "Shot ID or range (e.g. 0005 or 0001-0005)"
                    },
                    "range": {
                        "type": "string",
                        "description": "Time range filter (format: START-END)"
                    },
                    "smoke": {
                        "type": "boolean",
                        "description": "Use smoke profile (608x352, 10 steps) instead of prod"
                    },
                    "force": {
                        "type": "boolean",
                        "description": "Force overwrite and regenerate video (backs up previous to history/)"
                    },
                    "force_video_only": {
                        "type": "boolean",
                        "description": "Regenerate video using existing prompt.h3.txt without recompiling prompt.ko.md"
                    },
                    "mock_h3": {
                        "type": "boolean",
                        "description": "Use local mock H3 synthesis for offline testing"
                    }
                }
            }
        }),
        json!({
            "name": "assemble_movie",
            "description": "Assemble rendered shots into final movie (dual audio tracks) and side-by-side 1:1 compare video",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "proj_dir": {
                        "type": "string",
                        "description": "Project directory path (default: .)"
                    },
                    "smoke": {
                        "type": "boolean",
                        "description": "Assemble smoke profile instead of prod"
                    }
                }
            }
        }),
        json!({
            "name": "pool_status",
            "description": "Check DGX Spark server pool nodes, ports, remote workspace, and active endpoints",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "proj_dir": {
                        "type": "string",
                        "description": "Project directory path (default: .)"
                    }
                }
            }
        })
    ]
}

fn normalize_shot_id(shot_id: &str) -> String {
    let clean = shot_id.trim();
    if clean.starts_with("shot_") {
        clean.to_string()
    } else if let Ok(num) = clean.parse::<u32>() {
        format!("shot_{:04}", num)
    } else {
        clean.to_string()
    }
}

fn execute_tool(name: &str, args: &Value) -> Result<String> {
    match name {
        "decompile_movie" => {
            let input = args
                .get("input")
                .and_then(|v| v.as_str())
                .context("Parameter 'input' is required for decompile_movie")?;
            let output = args.get("output").and_then(|v| v.as_str());
            let range = args.get("range").and_then(|v| v.as_str());
            let max_shot_duration = args.get("max_shot_duration").and_then(|v| v.as_f64());
            let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
            let run = args.get("run").and_then(|v| v.as_bool()).unwrap_or(false);
            let smoke = args.get("smoke").and_then(|v| v.as_bool()).unwrap_or(false);
            let offline = args.get("offline").and_then(|v| v.as_bool()).unwrap_or(false);
            let dub_lang = args.get("dub_lang").and_then(|v| v.as_str());
            let custom_mod = args.get("custom_mod").and_then(|v| v.as_str());

            let opts = DecompileOptions {
                input_path: input,
                output_dir: output,
                range,
                max_shot_duration,
                force,
                run_after: run,
                smoke,
                offline,
                dub_lang,
                custom_mod,
            };
            decompile::run_decompile(opts)?;
            Ok(format!("Decompiled '{}' successfully", input))
        }
        "list_shots" => {
            let proj_dir_str = args
                .get("proj_dir")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let proj_dir = Path::new(proj_dir_str);

            let timeline_path = proj_dir.join("timeline.json");
            let mut shots_summary = Vec::new();

            if timeline_path.is_file() {
                let manifest = TimelineManifest::load_from_file(&timeline_path)?;
                for shot in manifest.shots {
                    let shot_dir = proj_dir.join("shots").join(&shot.shot_id);
                    let prod_exists = shot_dir
                        .join("renders")
                        .join("prod")
                        .join(format!("{}.mp4", shot.shot_id))
                        .is_file();
                    let smoke_exists = shot_dir
                        .join("renders")
                        .join("smoke")
                        .join(format!("{}.mp4", shot.shot_id))
                        .is_file();

                    shots_summary.push(json!({
                        "shot_id": shot.shot_id,
                        "start_sec": shot.start_sec,
                        "end_sec": shot.end_sec,
                        "duration": shot.duration,
                        "anchor": shot.anchor,
                        "prod_rendered": prod_exists,
                        "smoke_rendered": smoke_exists,
                    }));
                }
            } else {
                let shots_dir = proj_dir.join("shots");
                if shots_dir.is_dir() {
                    for entry in fs::read_dir(&shots_dir)?.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                                if name.starts_with("shot_") {
                                    let meta_file = path.join("shot.toml");
                                    let meta = ShotMeta::load_from_file(&meta_file).ok();
                                    let prod_exists = path
                                        .join("renders")
                                        .join("prod")
                                        .join(format!("{}.mp4", name))
                                        .is_file();
                                    let smoke_exists = path
                                        .join("renders")
                                        .join("smoke")
                                        .join(format!("{}.mp4", name))
                                        .is_file();

                                    shots_summary.push(json!({
                                        "shot_id": name,
                                        "start_sec": meta.as_ref().map(|m| m.start_time),
                                        "end_sec": meta.as_ref().map(|m| m.end_time),
                                        "duration": meta.as_ref().map(|m| m.duration),
                                        "anchor": meta.as_ref().map(|m| m.anchor.clone()).unwrap_or_else(|| "unknown".to_string()),
                                        "prod_rendered": prod_exists,
                                        "smoke_rendered": smoke_exists,
                                    }));
                                }
                            }
                        }
                    }
                }
            }

            Ok(serde_json::to_string_pretty(&shots_summary)?)
        }
        "inspect_shot" => {
            let shot_id_raw = args
                .get("shot_id")
                .and_then(|v| v.as_str())
                .context("Parameter 'shot_id' is required for inspect_shot")?;
            let shot_id = normalize_shot_id(shot_id_raw);
            let proj_dir_str = args
                .get("proj_dir")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let shot_dir = Path::new(proj_dir_str).join("shots").join(&shot_id);

            if !shot_dir.is_dir() {
                anyhow::bail!("Shot directory does not exist: {:?}", shot_dir);
            }

            let meta_path = shot_dir.join("shot.toml");
            let meta_content = fs::read_to_string(&meta_path).unwrap_or_default();

            let ko_path = shot_dir.join("prompt.ko.md");
            let ko_content = fs::read_to_string(&ko_path).unwrap_or_default();

            let h3_path = shot_dir.join("prompt.h3.txt");
            let h3_content = fs::read_to_string(&h3_path).unwrap_or_default();

            let res = json!({
                "shot_id": shot_id,
                "shot_toml": meta_content,
                "prompt_ko": ko_content,
                "prompt_h3": h3_content,
            });
            Ok(serde_json::to_string_pretty(&res)?)
        }
        "update_shot_prompt" => {
            let shot_id_raw = args
                .get("shot_id")
                .and_then(|v| v.as_str())
                .context("Parameter 'shot_id' is required for update_shot_prompt")?;
            let shot_id = normalize_shot_id(shot_id_raw);
            let proj_dir_str = args
                .get("proj_dir")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let shot_dir = Path::new(proj_dir_str).join("shots").join(&shot_id);

            if !shot_dir.is_dir() {
                anyhow::bail!("Shot directory does not exist: {:?}", shot_dir);
            }

            let meta_path = shot_dir.join("shot.toml");
            let mut meta = ShotMeta::load_from_file(&meta_path)?;

            let mut updated_fields = Vec::new();

            if let Some(new_anchor) = args.get("anchor").and_then(|v| v.as_str()) {
                meta.anchor = new_anchor.to_string();
                meta.save_to_file(&meta_path)?;
                updated_fields.push(format!("anchor -> {}", new_anchor));
            }

            let mut should_recompile = false;
            if let Some(ko) = args.get("prompt_ko").and_then(|v| v.as_str()) {
                fs::write(shot_dir.join("prompt.ko.md"), ko)?;
                updated_fields.push("prompt.ko.md".to_string());
                should_recompile = true;
            }

            if let Some(h3) = args.get("prompt_h3").and_then(|v| v.as_str()) {
                fs::write(shot_dir.join("prompt.h3.txt"), h3)?;
                updated_fields.push("prompt.h3.txt".to_string());
                should_recompile = false; // explicitly provided
            }

            if let Some(recomp) = args.get("recompile").and_then(|v| v.as_bool()) {
                should_recompile = recomp;
            }

            if should_recompile {
                compile_prompt_file(&shot_dir, &meta)?;
                updated_fields.push("recompiled prompt.h3.txt".to_string());
            }

            Ok(format!(
                "Shot {} updated successfully: {}",
                shot_id,
                updated_fields.join(", ")
            ))
        }
        "render_shot" => {
            let proj_dir_str = args
                .get("proj_dir")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let proj_dir = Path::new(proj_dir_str);
            let shot = args.get("shot").and_then(|v| v.as_str()).map(String::from);
            let range = args
                .get("range")
                .and_then(|v| v.as_str())
                .map(String::from);
            let smoke = args.get("smoke").and_then(|v| v.as_bool()).unwrap_or(false);
            let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
            let force_video_only = args
                .get("force_video_only")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mock_h3 = args
                .get("mock_h3")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let opts = RenderOptions {
                proj_dir,
                shot,
                range,
                smoke,
                force,
                force_video_only,
                mock_h3,
            };
            render::run_render(opts)?;
            Ok("Render finished successfully".to_string())
        }
        "assemble_movie" => {
            let proj_dir_str = args
                .get("proj_dir")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let proj_dir = Path::new(proj_dir_str);
            let smoke = args.get("smoke").and_then(|v| v.as_bool()).unwrap_or(false);

            let opts = AssembleOptions { proj_dir, smoke };
            assemble::run_assemble(opts)?;
            Ok("Movie assembly finished successfully".to_string())
        }
        "pool_status" => {
            let proj_dir_str = args
                .get("proj_dir")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let proj_dir = Path::new(proj_dir_str);

            let config_path = proj_dir.join("rea-movie.toml");
            let config = if config_path.is_file() {
                ProjectConfig::load_from_file(&config_path)?
            } else {
                ProjectConfig::default()
            };

            let endpoints = pool::discover_endpoints(&config.pool);
            let endpoints_json = endpoints
                .iter()
                .map(|ep| {
                    json!({
                        "host": ep.host,
                        "port": ep.port,
                        "engine": ep.engine.map(|e| e.as_str()),
                        "url": ep.base_url(),
                    })
                })
                .collect::<Vec<_>>();

            let res = json!({
                "configured_nodes": config.pool.nodes,
                "base_port": config.pool.base_port,
                "remote_workspace": config.pool.remote_workspace,
                "active_endpoints": endpoints_json,
            });
            Ok(serde_json::to_string_pretty(&res)?)
        }
        _ => anyhow::bail!("Unknown tool: {}", name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_shot_id() {
        assert_eq!(normalize_shot_id("1"), "shot_0001");
        assert_eq!(normalize_shot_id("0005"), "shot_0005");
        assert_eq!(normalize_shot_id("shot_0012"), "shot_0012");
    }

    #[test]
    fn test_mcp_tools_list_definition() {
        let tools = get_tool_definitions();
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
            .collect();
        assert!(names.contains(&"decompile_movie"));
        assert!(names.contains(&"list_shots"));
        assert!(names.contains(&"inspect_shot"));
        assert!(names.contains(&"update_shot_prompt"));
        assert!(names.contains(&"render_shot"));
        assert!(names.contains(&"assemble_movie"));
        assert!(names.contains(&"pool_status"));
        assert_eq!(names.len(), 7);
    }

    #[test]
    fn test_handle_initialize_and_tools_list() {
        let init_req = JsonRpcRequest {
            jsonrpc: Some("2.0".to_string()),
            id: Some(json!(1)),
            method: "initialize".to_string(),
            params: None,
        };
        let init_resp = handle_request(&init_req).expect("Expected initialize response");
        assert_eq!(init_resp["id"], 1);
        assert_eq!(init_resp["result"]["serverInfo"]["name"], "rea-movie");

        let tools_req = JsonRpcRequest {
            jsonrpc: Some("2.0".to_string()),
            id: Some(json!(2)),
            method: "tools/list".to_string(),
            params: None,
        };
        let tools_resp = handle_request(&tools_req).expect("Expected tools/list response");
        assert_eq!(tools_resp["id"], 2);
        let list = tools_resp["result"]["tools"].as_array().unwrap();
        assert_eq!(list.len(), 7);
    }
}
