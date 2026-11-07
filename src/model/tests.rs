use super::*;

#[test]
fn to_i64s_widens_each_value() {
    assert_eq!(to_i64s(&[1, 2, 3]), vec![1i64, 2, 3]);
}

#[test]
fn to_i64s_keeps_values_above_the_i32_range() {
    assert_eq!(to_i64s(&[u32::MAX]), vec![4_294_967_295i64]);
}

#[test]
fn to_i64s_of_nothing_is_empty() {
    assert!(to_i64s(&[]).is_empty());
}

#[test]
fn create_array_has_one_row_of_the_given_width() {
    match create_array(vec![10, 20, 30], 3) {
        Ok(array) => {
            assert_eq!(array.shape(), &[1, 3]);
            assert_eq!(array[[0, 0]], 10);
            assert_eq!(array[[0, 2]], 30);
        }
        Err(err) => panic!("unexpected error: {err}"),
    }
}

#[test]
fn create_array_rejects_a_width_that_does_not_match_the_data() {
    assert!(matches!(
        create_array(vec![1, 2, 3], 2),
        Err(Error::ShapeError(_))
    ));
}

#[test]
fn model_file_joins_the_file_name_onto_the_model_root() {
    let model = SimpleModel::new(Path::new("/models/minilm"));

    assert_eq!(
        model.model_file("model.onnx"),
        PathBuf::from("/models/minilm/model.onnx")
    );
    assert_eq!(
        model.model_file("tokenizer.json"),
        PathBuf::from("/models/minilm/tokenizer.json")
    );
}

#[test]
fn default_model_root_is_the_bundled_minilm_directory() {
    let model = SimpleModel::default();

    assert!(model.model_root.ends_with("models/all-MiniLM-L6-v2-onnx"));
}
