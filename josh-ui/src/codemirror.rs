use super::*;

#[derive(Properties, Clone, PartialEq)]
pub struct Props {
    pub text: String,
    pub marker_pos: Vec<i64>,
    pub marker_text: Vec<String>,
}

pub struct Codemirror {
    _link: ComponentLink<Self>,
    props: Props,
}

impl Component for Codemirror {
    type Message = ();
    type Properties = Props;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self { _link: link, props }
    }

    fn rendered(&mut self, first_render: bool) {
        if first_render {
            init_codemirror();
        }
        set_codemirror(&self.props.text);

        for (pos, text) in self
            .props
            .marker_pos
            .iter()
            .zip(self.props.marker_text.iter())
        {
            set_marker(*pos, &text);
        }
    }

    fn update(&mut self, _msg: Self::Message) -> ShouldRender {
        false
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        if self.props != props {
            self.props = props;
        }
        return true;
    }

    fn view(&self) -> Html {
        html! { <div class="filemode loaded" id="codeview"/> }
    }
}

#[wasm_bindgen(module = "/src/cm.js")]
extern "C" {
    #[wasm_bindgen(js_name = "initCodemirror")]
    pub fn init_codemirror();

    #[wasm_bindgen(js_name = "setCodemirror")]
    pub fn set_codemirror(text: &str);

    #[wasm_bindgen(js_name = "setMarker")]
    pub fn set_marker(position: i64, text: &str);
}
