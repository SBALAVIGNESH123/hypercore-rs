use walkdir::DirEntry;

pub struct RawChunk {
    pub text: String,
    pub chunk_index: usize,
    pub start_offset: usize,
    pub end_offset: usize,
    pub file_extension: String,
}

pub fn is_supported(entry: &DirEntry) -> Option<String> {
    let extensions = ["txt", "md", "rs", "py", "js", "ts", "json", "yaml", "c", "cpp", "h", "hpp", "cu", "cuh"];
    if entry.file_type().is_file() {
        if let Some(ext) = entry.path().extension() {
            if let Some(ext_str) = ext.to_str() {
                if extensions.contains(&ext_str) {
                    return Some(ext_str.to_string());
                }
            }
        }
    }
    None
}

// Old sliding window fallback
pub fn chunk_text_fallback(text: &str, chunk_size: usize, overlap: usize, file_extension: &str) -> Vec<RawChunk> {
    let mut chunks = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut chunk_index = 0;

    while i < chars.len() {
        let end = std::cmp::min(i + chunk_size, chars.len());
        let chunk_str: String = chars[i..end].iter().collect();
        chunks.push(RawChunk {
            text: chunk_str,
            chunk_index,
            start_offset: i,
            end_offset: end,
            file_extension: file_extension.to_string(),
        });
        if end == chars.len() {
            break;
        }
        i += chunk_size - overlap;
        chunk_index += 1;
    }
    chunks
}

pub fn chunk_text(text: &str, file_path: &str, chunk_size: usize, overlap: usize, file_extension: &str) -> Vec<RawChunk> {
    let mut parser = tree_sitter::Parser::new();
    let language = match file_extension {
        "c" | "h" => tree_sitter_c::language(),
        "cpp" | "hpp" | "cu" | "cuh" => tree_sitter_cpp::language(),
        _ => return chunk_text_fallback(text, chunk_size, overlap, file_extension),
    };

    if parser.set_language(&language.into()).is_err() {
        return chunk_text_fallback(text, chunk_size, overlap, file_extension);
    }

    let tree = if let Some(tree) = parser.parse(text, None) {
        tree
    } else {
        return chunk_text_fallback(text, chunk_size, overlap, file_extension);
    };

    let mut chunks = Vec::new();
    let mut cursor = tree.walk();
    let mut chunk_index = 0;
    let mut function_nodes = Vec::new();

    // Collect all function_definition nodes
    fn traverse<'a>(cursor: &mut tree_sitter::TreeCursor<'a>, function_nodes: &mut Vec<tree_sitter::Node<'a>>) {
        let node = cursor.node();
        if node.kind() == "function_definition" {
            function_nodes.push(node);
        }
        if cursor.goto_first_child() {
            traverse(cursor, function_nodes);
            while cursor.goto_next_sibling() {
                traverse(cursor, function_nodes);
            }
            cursor.goto_parent();
        }
    }

    traverse(&mut cursor, &mut function_nodes);

    if function_nodes.is_empty() {
        return chunk_text_fallback(text, chunk_size, overlap, file_extension);
    }

    for node in function_nodes {
        let start_byte = node.start_byte();
        let end_byte = node.end_byte();
        let node_text = &text[start_byte..end_byte];
        
        // Truncate to signature + first 5 lines
        let lines: Vec<&str> = node_text.lines().collect();
        let mut limit = 0;
        let mut in_body = false;
        let mut truncated_text = String::new();
        
        for line in lines {
            truncated_text.push_str(line);
            truncated_text.push('\n');
            if line.contains('{') {
                in_body = true;
            }
            if in_body {
                limit += 1;
            }
            if limit >= 5 {
                truncated_text.push_str("    // ... truncated body\n");
                if !truncated_text.contains('}') {
                    truncated_text.push_str("}\n");
                }
                break;
            }
        }
        
        let final_text = format!("// File: {}\n{}", file_path, truncated_text);
        
        chunks.push(RawChunk {
            text: final_text,
            chunk_index,
            start_offset: start_byte,
            end_offset: end_byte,
            file_extension: file_extension.to_string(),
        });
        chunk_index += 1;
    }
    
    chunks
}
