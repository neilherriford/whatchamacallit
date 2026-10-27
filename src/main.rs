use ndarray::{Array2, ArrayBase, Dim, OwnedRepr};
use ort::{
    session::{
        Session,
        builder::{GraphOptimizationLevel, SessionBuilder},
    },
    value::Tensor,
};
use std::path::PathBuf;
use thiserror::Error;
use tokenizers::Tokenizer;

#[derive(Error, Debug)]
pub enum Error {
    #[error("ORT error: {0}")]
    OrtError(#[from] ort::Error),
    #[error("Session builder error: {0}")]
    SessionBulderError(#[from] ort::Error<SessionBuilder>),
    #[error("Shape error: {0}")]
    ShapeError(#[from] ndarray::ShapeError),
    #[error("General error: {0}")]
    GeneralError(#[from] Box<dyn std::error::Error + Send + Sync>),
}

fn main() {
    match run("a very cool definition for a word i can't remember") {
        Ok(_) => println!("done!"),
        Err(err) => println!("oops {err}"),
    }
}

fn model_file(file_name: &str) -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "models",
        "all-MiniLM-L6-v2-onnx",
        file_name,
    ]
    .iter()
    .collect()
}

fn create_array(
    ids: Vec<i64>,
    size: usize,
) -> Result<ArrayBase<OwnedRepr<i64>, Dim<[usize; 2]>, i64>, Error> {
    Ok(Array2::from_shape_vec((1, size), ids)?)
}

fn run(prompt: &str) -> Result<(), Error> {
    let env = ort::init().build()?;

    let mut model = Session::builder(&env)?
        .with_optimization_level(GraphOptimizationLevel::Level1)?
        .with_intra_threads(4)?
        .commit_from_file(model_file("model.onnx"))?;

    let tokenizer = Tokenizer::from_file(model_file("tokenizer.json"))?;
    let tokens = tokenizer.encode(prompt, false).unwrap();

    let ids = tokens
        .get_ids()
        .iter()
        .map(|i| *i as i64)
        .collect::<Vec<_>>();

    let attention_mask = tokens
        .get_attention_mask()
        .iter()
        .map(|i| *i as i64)
        .collect::<Vec<_>>();

    let type_ids = tokens
        .get_type_ids()
        .iter()
        .map(|i| *i as i64)
        .collect::<Vec<_>>();

    let tensor_ids = Tensor::from_array(create_array(ids, tokens.len())?)?;
    let tensor_mask = Tensor::from_array(create_array(attention_mask.clone(), tokens.len())?)?;
    let tensor_types = Tensor::from_array(create_array(type_ids, tokens.len())?)?;

    let outputs = model.run(ort::inputs![
        "input_ids" => tensor_ids,
        "attention_mask" => tensor_mask,
        "token_type_ids" => tensor_types,
    ])?;

    let (shape, data) = outputs[0].try_extract_tensor::<f32>()?;

    println!("The exact output shape is: {:?}", shape);
    println!("The flattened data has {} total floats", data.len());

    let mut acc: Vec<f32> = vec![0.0; 384];
    let mut valid_tokens_count: f32 = 0.0;

    for (chunk, mask) in data.chunks_exact(384).zip(attention_mask) {
        if mask == 0 {
            continue;
        }
        valid_tokens_count += 1.0;
        for (index, value) in chunk.iter().enumerate() {
            acc[index] += value;
        }
    }

    let result = acc
        .iter()
        .map(|val| val / valid_tokens_count)
        .collect::<Vec<_>>();

    println!("output: {result:?}");
    Ok(())
}
