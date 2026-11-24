use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = compilerVersion)]
pub fn compiler_version() -> String {
    crate::compiler_version().into()
}

#[wasm_bindgen(js_name = analyzeJson)]
pub fn analyze_json(input: js_sys::JsString) -> Result<js_sys::JsString, JsValue> {
    invoke_json(input, crate::analyze_json)
}

#[wasm_bindgen(js_name = parseJson)]
pub fn parse_json(input: js_sys::JsString) -> Result<js_sys::JsString, JsValue> {
    invoke_json(input, crate::parse_json)
}

#[wasm_bindgen]
pub fn compile(input: JsValue) -> Result<JsValue, JsValue> {
    let request = js_sys::JSON::stringify(&input)?;
    let report = invoke_json(request, crate::compile_json)?;
    js_sys::JSON::parse(&String::from(report))
}

fn invoke_json(
    input: js_sys::JsString,
    operation: fn(&str) -> Result<String, crate::RequestError>,
) -> Result<js_sys::JsString, JsValue> {
    if !input.is_string() {
        return Err(JsValue::from_str(
            "REQUEST_STRING: Expected a JavaScript string",
        ));
    }
    if input.length() > crate::wire::MAX_WIRE_BYTES as u32 {
        return Err(JsValue::from_str(
            "REQUEST_WIRE_SIZE: JSON input exceeds 16 MiB",
        ));
    }
    if !input.is_valid_utf16() {
        return Err(JsValue::from_str(
            "REQUEST_UTF16: Unpaired JavaScript surrogate",
        ));
    }
    let text = input
        .as_string()
        .ok_or_else(|| JsValue::from_str("REQUEST_STRING: Expected a JavaScript string"))?;
    operation(&text)
        .map(|s| s.into())
        .map_err(|error| JsValue::from_str(&error.to_string()))
}
