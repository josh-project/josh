import React, { useState, useEffect } from "react";
import "./App.css";

const useFetch = (url) => {
  const [data, updateData] = useState({"diff":""});

  // empty array as second argument equivalent to componentDidMount
  useEffect(() => {
    async function fetchData() {
      const response = await fetch(url);
      const json = await response.json();
      updateData(json);
    }
    fetchData();
  }, [url]);

  return data;
};

function CodeLine(cls, oline, nline, value) {
  return (
    <tr class={cls}>
      <td class="linenr">
        <pre>{oline}</pre>
      </td>
      <td class="linenr">
        <pre>{nline}</pre>
      </td>
      <td class="code">
        <pre>{value}</pre>
      </td>
    </tr>
  );
}

const comments = {
  cc8f4b1e5d: {
    27: {
      text: "templates are evil\nthe devil made them",
      timestamp: "Mar 27 12:58 PM",
    },
  },
};

function DiffUnified(props) {

  var current_ofile = "";
  var current_nfile = "";

  var arr = [];
  var result = props.diff.split("\n")

  var oline = 0;
  var nline = 0;
  for (var i = 0; i < result.length; ++i) {
    const value = result[i];
    if (value.startsWith("@")) {
      continue;
    }
    if (value.startsWith("+++")) {
      continue;
    }
    if (value.startsWith("---")) {
      continue;
    }
    if (value.startsWith("index")) {
      const [idx, ff, rest] = value.split(" ");
      [current_ofile, current_nfile] = ff.split("..");
      continue;
    }
    if (value.startsWith("diff --git")) {
      oline = 1;
      nline = 1;
      const [d, g, a, b] = value.split(" ");
      arr.push(
        <tr class="head">
          <td colspan="3">
            <pre>&nbsp;</pre>
          </td>
        </tr>
      );
      arr.push(
        <tr class="head">
          <td colspan="3">
            <pre>
              {a.substring(2)} -> {b.substring(2)}
            </pre>
          </td>
        </tr>
      );
      arr.push(
        <tr class="head">
          <td colspan="3">
            <pre>&nbsp;</pre>
          </td>
        </tr>
      );
      continue;
    }
    if (value.startsWith(" ")) {
      arr.push(CodeLine("", oline, nline, value.substring(1)));
      ++oline;
      ++nline;
    } else if (value.startsWith("+")) {
      arr.push(CodeLine("addition", "", nline, value.substring(1)));
      ++nline;
    } else if (value.startsWith("-")) {
      arr.push(CodeLine("removal", oline, "", value.substring(1)));
      ++oline;
    }

    if (current_nfile in comments) {
      const cf = comments[current_nfile];
      if (nline - 1 in cf) {
        const comment = cf[nline - 1];
        arr.push(CodeLine("comment", "", "", comment.text));
      }
    }
  }

  return (
    <table>
      <tbody>{arr}</tbody>
    </table>
  );
}

function App() {
  const [prefix, id] = window.location.pathname
    .split("/")
    .filter((x) => x != "");
  const result = useFetch("/c/" + id + "/")["diff"];

    return <DiffUnified diff={result}/>;
}

export default App;
