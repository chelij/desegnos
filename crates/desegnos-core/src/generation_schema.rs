use crate::limits::{
    MAX_DECISION_PROPOSALS, MAX_DESIGN_CRITICISMS, MAX_EXPLANATION_CHARS, MAX_GENERATED_IMAGES,
    MAX_GENERATION_REQUESTS, MAX_POINTER_CLASSIFICATIONS,
};
pub(crate) fn uuid_schema() -> serde_json::Value {
    serde_json::json!({"type":"string","pattern":"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$"})
}

fn nullable<T: serde::Serialize>(schema: T) -> serde_json::Value {
    serde_json::json!({"anyOf":[schema,{"type":"null"}]})
}

pub fn candidate_schema() -> serde_json::Value {
    use serde_json::json;
    fn object(properties: serde_json::Value) -> serde_json::Value {
        let required: Vec<_> = properties
            .as_object()
            .expect("schema properties are objects")
            .keys()
            .cloned()
            .collect();
        json!({"type":"object","additionalProperties":false,"required":required,"properties":properties})
    }
    fn text(maximum: usize) -> serde_json::Value {
        json!({"type":"string","description":format!("At most {maximum} Unicode characters, enforced by Desegnos before acceptance.")})
    }
    fn array(items: serde_json::Value, maximum: usize) -> serde_json::Value {
        json!({"type":"array","maxItems":maximum,"items":items})
    }
    let uuid = uuid_schema();
    let colour = json!({"type":"string","description":"#rgb, #rrggbb or #rrggbbaa; transparent is allowed."});
    let stroke = object(
        json!({"color":nullable(colour.clone()),"width":{"type":"number","minimum":0,"maximum":256}}),
    );
    let shadow = object(
        json!({"x":{"type":"number"},"y":{"type":"number"},"blur":{"type":"number","minimum":0,"maximum":4096},"color":nullable(colour.clone())}),
    );
    let text_style = object(json!({
        "fontFamily":text(200),"size":{"type":"number","minimum":1,"maximum":512},
        "weight":{"type":"integer","minimum":1,"maximum":1000},"italic":{"type":"boolean"},
        "lineHeight":{"type":"number","minimum":0,"maximum":10},
        "letterSpacing":{"type":"number","minimum":-100,"maximum":100},
        "color":colour.clone(),"align":{"type":"string","enum":["left","center","right"]}
    }));
    let style = object(json!({
        "fill":nullable(colour),"stroke":stroke,"radius":{"type":"number","minimum":0},
        "opacity":{"type":"number","minimum":0,"maximum":1},"shadow":shadow,"textStyle":text_style
    }));
    let rect = object(
        json!({"x":{"type":"number"},"y":{"type":"number"},"width":{"type":"number","minimum":0},"height":{"type":"number","minimum":0}}),
    );
    let layout = json!({"anyOf":[
        object(json!({"kind":{"type":"string","enum":["free"]}})),
        object(json!({"kind":{"type":"string","enum":["stack"]},"direction":{"type":"string","enum":["row","column"]},"gap":{"type":"number"},"padding":{"type":"number"},"align":{"type":"string","enum":["start","center","end","stretch"]},"justify":{"type":"string","enum":["start","center","end","spaceBetween"]},"wrap":{"type":"boolean"}})),
        object(json!({"kind":{"type":"string","enum":["grid"]},"columns":{"type":"integer","minimum":1,"maximum":64},"gap":{"type":"number"},"padding":{"type":"number"}}))
    ]});
    let kind = json!({"type":"string","enum":["box","group","text","ellipse","line","image","button","link","input","textarea","select","checkbox","radio","video","audio","embed","code"]});
    let input_type = json!({"type":"string","enum":["text","email","password","number","search","tel","url","date","time","color","range","file"]});
    // Strict structured output requires every property of every object, so a field the reply may
    // omit is required and reads null as omitted. The element kind fields stay nullable because
    // the flattened kind decides which of them applies.
    let draft = object(json!({
        "kind":kind,"rect":rect,"name":nullable(text(120)),
        "rotation":{"type":"number"},"visible":{"type":"boolean"},"locked":{"type":"boolean"},
        "style":style,"text":text(16_384),"image":nullable(uuid.clone()),
        "imageRequest":nullable(text(120)),"url":nullable(text(2048)),"layout":layout,
        "inputType":nullable(input_type.clone()),
        "options":nullable(json!({"type":"array","minItems":1,"maxItems":100,"items":text(120)})),
        "language":nullable(text(40)),"source":nullable(text(200_000)),
        "children":{"type":"array","maxItems":512,"items":{"$ref":"#/$defs/element"}}
    }));
    let update = object(json!({
        "elementId":uuid.clone(),"rect":nullable(rect.clone()),"rotation":nullable(json!({"type":"number"})),
        "style":nullable(style),"text":nullable(text(16_384)),"url":nullable(text(2048)),
        "layout":nullable(layout),"visible":nullable(json!({"type":"boolean"})),
        "locked":nullable(json!({"type":"boolean"})),"name":nullable(text(120)),
        "inputType":nullable(input_type),
        "options":nullable(json!({"type":"array","maxItems":100,"items":text(120)})),
        "language":nullable(text(40)),"source":nullable(text(200_000)),
        "sizeId":{"anyOf":[uuid.clone(),{"type":"null"}],"description":"Change only this linked size of the boundary; null keeps the base values every size inherits."}
    }));
    let reference_kind = json!({"anyOf":[
        object(json!({"kind":{"type":"string","enum":["note"]},"text":text(16_384)})),
        object(json!({"kind":{"type":"string","enum":["link"]},"url":text(2048),"title":text(120)})),
        object(json!({"kind":{"type":"string","enum":["swatch"]},"color":text(32)}))
    ]});
    let operation = json!({"anyOf":[
        object(json!({"kind":{"type":"string","enum":["replaceChildren"]},"children":{"type":"array","maxItems":512,"items":{"$ref":"#/$defs/element"}}})),
        object(json!({"kind":{"type":"string","enum":["update"]},"patches":{"type":"array","minItems":1,"maxItems":2048,"items":update}})),
        object(json!({"kind":{"type":"string","enum":["insert"]},"parentId":uuid.clone(),"index":nullable(json!({"type":"integer","minimum":0})),"element":{"$ref":"#/$defs/element"}})),
        object(json!({"kind":{"type":"string","enum":["remove"]},"elementId":uuid.clone()})),
        object(json!({"kind":{"type":"string","enum":["setBehaviour"]},"elementId":nullable(uuid.clone()),"behaviour":text(crate::limits::MAX_PROMPT_CHARS)})),
        object(json!({"kind":{"type":"string","enum":["insertInBoundary"]},"pageId":uuid.clone(),"sizeId":nullable(uuid.clone()),"element":{"$ref":"#/$defs/element"}})),
        object(json!({"kind":{"type":"string","enum":["setPage"]},"pageId":uuid.clone(),"name":nullable(json!({"type":"string","minLength":1,"maxLength":120})),"path":nullable(json!({"type":"string","minLength":1,"maxLength":1024})),"background":nullable(json!({"type":"string","pattern":"^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$"}))})),
        object(json!({"kind":{"type":"string","enum":["addReference"]},"item":object(json!({"rect":rect,"kind":reference_kind}))}))
    ]});
    let action = json!({"anyOf":[
        object(json!({"kind":{"type":"string","enum":["goTo"]},"boundary":uuid.clone()})),
        object(json!({"kind":{"type":"string","enum":["show"]},"element":uuid.clone()})),
        object(json!({"kind":{"type":"string","enum":["hide"]},"element":uuid.clone()})),
        object(json!({"kind":{"type":"string","enum":["toggle"]},"element":uuid.clone()})),
        object(json!({"kind":{"type":"string","enum":["swap"]},"element":uuid.clone(),"withElement":uuid.clone()})),
        object(json!({"kind":{"type":"string","enum":["exportOnly"]}}))
    ]});
    let scope = json!({"anyOf":[
        object(json!({"kind":{"type":"string","enum":["project"]}})),
        object(json!({"kind":{"type":"string","enum":["page"]},"id":uuid.clone()})),
        object(json!({"kind":{"type":"string","enum":["element"]},"id":uuid.clone()}))
    ]});
    let schema = object(json!({
        "jobId":uuid,
        "requirementsSha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
        "summary":text(16_384),
        "requests":array(object(json!({
            "requestId":nullable(uuid.clone()),
            "elementId":nullable(uuid.clone()),
            "status":{"type":"string","enum":["implemented","conflict"]},
            "explanation":text(MAX_EXPLANATION_CHARS),
            "operations":array(operation,2048)
        })),MAX_GENERATION_REQUESTS),
        "images":array(object(json!({
            "key":text(120),"prompt":text(4000),
            "width":{"type":"integer","minimum":16,"maximum":4096},
            "height":{"type":"integer","minimum":16,"maximum":4096}
        })),MAX_GENERATED_IMAGES),
        "pointerClassifications":array(object(json!({
            "pointerId":uuid.clone(),"comment":text(16_384),"action":action
        })),MAX_POINTER_CLASSIFICATIONS),
        "decisionProposals":array(object(json!({
            "topic":text(120),"value":text(16_384),"scope":scope,"rationale":text(MAX_EXPLANATION_CHARS)
        })),MAX_DECISION_PROPOSALS),
        "criticism":array(object(json!({
            "elementId":nullable(uuid.clone()),
            "message":text(MAX_EXPLANATION_CHARS),
            "suggestion":text(MAX_EXPLANATION_CHARS)
        })),MAX_DESIGN_CRITICISMS)
    }));
    let mut schema = schema;
    schema["$defs"] = json!({"element": draft});
    schema
}

pub fn result_schema() -> serde_json::Value {
    let mut schema = candidate_schema();
    schema["properties"]["requests"]["description"] = serde_json::json!(
        "Deliver every requested layer exactly once. A conflict must not withhold other ready layers."
    );
    schema["properties"]["pointerClassifications"]["description"] = serde_json::json!(
        "Classify every pointer listed under pointer comments to classify, and no other. Copy its current comment exactly. Leave empty when none are listed."
    );
    schema
}
