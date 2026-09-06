use std::{fs::File, io::Cursor, path::{PathBuf, Path}};

use anyhow::Result;
use clap::Parser;
use mh1tool::{amo::{AmoNode, AmoView, enumerate_amo, export_to_gltf}, amo_ahi_expand, apx::{self}, get_link_file_slice, load_texlist, meltw, read_afs_file, read_all_links};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    input: String,
    #[arg(short, long)]
    output: PathBuf,
    #[arg(short, long)]
    dump_amo_tex: bool,
    #[arg(short, long)]
    dump: Option<PathBuf>,
}

fn get_output_name(input: &str, output: &Path) -> PathBuf {
    let mut out = PathBuf::from(output);
    if output.is_dir() {
        let input = PathBuf::from(input);
        let file_name = input.file_name().unwrap();
        out.push(file_name);
    }
    out
}

fn read_tex_bin_data(data: &[u8], output: PathBuf) -> Result<Vec<u8>> {
    let tex_list = load_texlist(data).expect("Failed to load tex list");
    let mut reader = Cursor::new(data);
    let data = read_afs_file(&mut reader)?;
    let path = output.to_string_lossy();
    for (i, tex) in tex_list.iter().enumerate() {
        let path = path.strip_suffix(".bin")
            .unwrap_or(path.as_ref());
        let path = format!("{path}.{i}");
        tex.save_all(&path);
    }
    Ok(data)
}

fn read_amo_amh_bin_data(data: &[u8], input: &str, output: &Path ) -> Result<Vec<u8>> {
    let (amo, ahi) = amo_ahi_expand(data).expect("Failed to amo_ahi_expand");
    println!("\nparsing model {}", input);
    let amo_node = AmoNode::from_buf(amo, &mut 0, None, 0)?;
    let output = format!("{}", output.to_string_lossy());
    //amo_node.write_obj_model(&output)?;
    let amo_view = AmoView::try_from(amo_node)?;
    export_to_gltf(&amo_view, &output)?;
        //println!("{:#?}", amo_node);
        //let mut reader = Cursor::new(amo);
        //let chunk = Chunk::read(&mut Cursor::new(&mat))?;
        //println!("chunk {:#?}", chunk);
        //let chunk = Chunk::read(&mut Cursor::new(&mesh))?;
        //println!("chunk {:#?}", chunk);
        /*let dir = Dir::read(&mut Cursor::new(&mat))?;
          println!("dir {:#?}", dir);
          println!("count {:#?}", dir.count);*/
        /*println!("entry 9 (material){:#?}", dir.values[9]);
          println!("entry 10 (texture){:#?}", dir.values[10] );*/
    Ok(data.to_vec())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let compressed_data = std::fs::read(&args.input)?;
    let data = meltw(&compressed_data).expect("Failed to meltw");

    let output = get_output_name(&args.input, &args.output);

    let data = if args.input.ends_with("_tex.bin") {
        read_tex_bin_data(&data, output)?
    } else if args.input.ends_with("_amh.bin") {
        let res = read_amo_amh_bin_data(&data, &args.input, &output)?;

        if args.dump_amo_tex {
            let clean_name = args.input.replace(".bin", "");
            let base_file_name = clean_name.replace("_amh", "");
            let tex_file_name = format!("{base_file_name}_tex.bin");

            let compressed_data = std::fs::read(&tex_file_name).unwrap();
            let tex_data = meltw(&compressed_data).expect("Failed to meltw");
            let output = get_output_name(&tex_file_name, &args.output);
            let _ = read_tex_bin_data(&tex_data, output)?;
        }

        res
    } else {
        data
    };
    if let Some(dump) = &args.dump {
        std::fs::write(dump, &data)?;
    }

    Ok(())
}
