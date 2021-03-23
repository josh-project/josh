
var editor = {};
export function initCodemirror() {
    var codeview = document.getElementById('codeview');
    editor = CodeMirror(codeview, {
        mode:  "clike",
        lineNumbers: true,
        readOnly: true,
        theme: "darcula",
        mode: "text/x-c++src",
    });
    editor.setSize("100%", "100%");
};

export function setCodemirror(text, markers) {
    editor.setSize("100%", "100%");
    editor.getDoc().setValue(text);
}

export function setMarker(position, text) {
    console.log([position, text]);
    var msg = document.createElement("div");
    msg.innerHTML = text;
    msg.className = "marker";
    editor.addLineWidget(parseInt(position)-1, msg, {coverGutter: false, noHScroll: true});
}
