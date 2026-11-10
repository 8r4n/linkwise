#!/usr/bin/env python3
"""Generate the template-tailored LaTeX test plan; --build also renders its PDF.

The maintained content is rust/docs/kiwisdr-test-plan.md. This deliberately
limited renderer handles that document's paragraphs, lists, tables, inline
markup, and code blocks; it is not a general Markdown conversion library.
"""
import argparse
import os
import shutil
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "rust/docs/kiwisdr-test-plan.md"
TARGET = ROOT / "docs/testing/kiwisdr-modem-test-plan.tex"


def escape(text):
    text = text.replace("–", "--").replace("—", "---")
    mapping = {"\\": r"\textbackslash{}", "&": r"\&", "%": r"\%",
               "$": r"\$", "#": r"\#", "_": r"\_", "{": r"\{",
               "}": r"\}", "~": r"\textasciitilde{}", "^": r"\textasciicircum{}"}
    return "".join(mapping.get(char, char) for char in text)


def inline(text):
    pattern = r"(\[[^\]]+\]\([^)]+\)|`[^`]+`|\*\*[^*]+\*\*)"
    result = []
    for part in re.split(pattern, text):
        link = re.fullmatch(r"\[([^\]]+)\]\(([^)]+)\)", part)
        if link:
            label, url = link.groups()
            if not url.startswith(("http://", "https://")):
                url = "../../../rust/docs/" + url
            result.append(r"\href{" + url.replace("%", r"\%") + "}{" + escape(label) + "}")
        elif part.startswith("`"):
            result.append(r"\path{" + part[1:-1] + "}")
        elif part.startswith("**"):
            result.append(r"\textbf{" + escape(part[2:-2]) + "}")
        else:
            result.append(escape(part))
    return "".join(result)


def plant_figure(name, caption, label):
    return (r"\landscapegraphic{../build/kiwisdr-test-plan/diagrams/" + name
            + "}{" + escape(caption) + "}{" + label + "}\n")


def tikz_figure(name, caption, label):
    return (r"\landscapetikz{figures/" + name + "}{" + escape(caption)
            + "}{" + label + "}\n")


DIAGRAM = plant_figure(
    "Kiwi_Measurement_Chain",
    "Proposed measurement chain. Kiwi receivers observe one-way RF; the expected-frame manifest is the independent oracle.",
    "fig:measurement-chain",
)


def render(text):
    lines = text.strip().splitlines()
    out, i = [], 0
    while i < len(lines):
        line = lines[i].strip()
        if not line:
            i += 1
            continue
        image_match = re.fullmatch(r"!\[([^\]]*)\]\(([^)]+)\)", line)
        if image_match:
            if image_match[2] != "../../docs/testing/figures/kiwisdr-candidate-map.png":
                raise ValueError("unrecognized publication map image")
            out.append(r"\landscapegraphic{figures/kiwisdr-candidate-map.pdf}{GIS candidate network centered on approximate Julian, California. Directory-derived sites were cross-checked through metadata only; geodesics are not propagation forecasts or confirmed links.}{fig:candidate-map}" + "\n")
            i += 1
            continue
        if line.startswith("```"):
            lang = line[3:]
            block = []
            i += 1
            while i < len(lines) and not lines[i].strip().startswith("```"):
                block.append(lines[i])
                i += 1
            if i == len(lines):
                raise ValueError("unclosed code block")
            i += 1
            if lang == "mermaid":
                # The publication figure follows the section narrative.
                pass
            else:
                out.append("\\begin{lstlisting}\n" + "\n".join(block) + "\n\\end{lstlisting}\n")
            continue
        if line.startswith("### "):
            title = re.sub(r"^\d+\.\d+\s+", "", line[4:])
            out.append(r"\subsubsection{" + inline(title) + "}\n")
            i += 1
            continue
        if line.startswith("|"):
            rows = []
            while i < len(lines) and lines[i].strip().startswith("|"):
                cells = [c.strip() for c in lines[i].strip().strip("|").split("|")]
                if not all(re.fullmatch(r":?-+:?", c) for c in cells):
                    rows.append(cells)
                i += 1
            n = len(rows[0])
            if any(len(row) != n for row in rows):
                raise ValueError("inconsistent table width")
            widths = {2: [.29, .71], 3: [.23, .54, .23],
                      4: [.18, .25, .32, .25]}[n]
            if rows[0][0].endswith(" step"):
                widths = [.10, .43, .29, .18]
            if rows[0][0] == "Candidate":
                widths = [.12, .40, .23, .25]
            if rows[0][0] == "Resource / role":
                widths = [.24, .12, .36, .28]
            captions = {
                "Item": "Test items and configuration evidence",
                "Phase": "Campaign matrix and experimental purpose",
                "Metric": "Metrics, denominators and implementation availability",
                "Objective": "Objective traceability and required evidence",
                "Resource / role": "Pilot resources, responsibilities and current readiness",
                "Event": "Dependency-based schedule; no calendar windows committed",
                "Baseline step": "A: local baseline and source identity",
                "Stimulus step": "B: native 30-burst preparation and exact verification",
                "Oracle review step": "B: oracle review and K0 stop condition",
                "Client step": "C: pinned recorder checkout",
                "Environment step": "C: capture environment and help verification",
                "Profile step": "D: receiver profile and readiness",
                "Capture step": "E: bounded capture and station execution sequence",
                "Completion step": "E: duration, evidence preservation and restoration",
                "Import step": "F: analysis dependency and audio format checks",
                "Conversion step": "F: declared audio conversion and Rust decode",
                "Result step": "F: exact RF comparison and report disposition",
                "Candidate": "Julian-centered receiver shortlist; metadata checked, access unconfirmed",
                "Selected template section": "Template crosswalk into the maintained Markdown draft",
            }
            caption = captions.get(rows[0][0], "Test-plan data")
            lead = ""
            if out and out[-1].startswith("\\subsubsection{"):
                lead = out.pop()
            out.append(r"\begin{landscape}\thispagestyle{plain}\centering" + "\n")
            if lead:
                out.append(lead + "\n")
            out.append(r"\setlength{\tabcolsep}{5pt}\renewcommand{\arraystretch}{1.25}" + "\n")
            out.append(r"\setlength{\tableavail}{\dimexpr\linewidth-" + str(2*(n-1)) + r"\tabcolsep\relax}" + "\n")
            cols = "".join(r">{\raggedright\arraybackslash}p{" + str(w) + r"\tableavail}" for w in widths)
            out.append(r"\begin{adjustbox}{max totalsize={0.96\linewidth}{0.76\textheight},center}" + "\n")
            out.append(r"\begin{tabular}{@{}" + cols + "@{}}" + "\n" + r"\toprule" + "\n")
            header_cells = (["Step"] + rows[0][1:]) if rows[0][0].endswith(" step") else rows[0]
            header = " & ".join(r"\textbf{" + inline(c) + "}" for c in header_cells) + r"\\"
            out.append(header + "\n" + r"\midrule" + "\n")
            for row in rows[1:]:
                out.append(" & ".join(inline(c) for c in row) + r"\\[5pt]" + "\n")
            out.append("\\bottomrule\n\\end{tabular}\n\\end{adjustbox}\n")
            out.append(r"\captionof{table}{" + escape(caption) + "}\n")
            out.append("\\end{landscape}\n")
            continue
        match = re.match(r"(- |\d+\. )(.*)", line)
        if match:
            env = "itemize" if match[1] == "- " else "enumerate"
            out.append("\\begin{" + env + "}\n")
            while i < len(lines):
                match = re.match(r"(- |\d+\. )(.*)", lines[i].strip())
                if not match or (match[1] == "- ") != (env == "itemize"):
                    break
                item = [match[2]]
                i += 1
                while i < len(lines) and lines[i].strip() and not re.match(r"(- |\d+\. |#|\||```|!\[)", lines[i].strip()):
                    item.append(lines[i].strip())
                    i += 1
                out.append(r"\item " + inline(" ".join(item)) + "\n")
            out.append("\\end{" + env + "}\n")
            continue
        paragraph = [line]
        i += 1
        while i < len(lines) and lines[i].strip() and not re.match(r"(- |\d+\. |#|\||```|!\[)", lines[i].strip()):
            paragraph.append(lines[i].strip())
            i += 1
        out.append(inline(" ".join(paragraph)) + "\n\n")
    return "".join(out)


PREAMBLE = r"""% Generated by tools/build-kiwisdr-test-plan.py; edit the Markdown source.
% Structure adapted from acq-latex-templates engineering/verification/test-plan.
% Upstream revision fef160e37542fd7bc8e62de81404a83b8634a396.
\documentclass[11pt,letterpaper]{article}
\usepackage[T1]{fontenc}
\usepackage{lmodern,microtype}
\usepackage[margin=0.85in,includeheadfoot,headheight=20pt]{geometry}
\usepackage{booktabs,longtable,array,needspace}
\usepackage{enumitem,fancyhdr,lastpage,parskip,xcolor}
\usepackage{listings,tikz,graphicx,adjustbox,pdflscape,caption}
\usetikzlibrary{arrows.meta}
\PassOptionsToPackage{obeyspaces}{url}
\usepackage[hidelinks,pdftitle={Linkwise Modem KiwiSDR Test Plan},pdfauthor={Linkwise Project}]{hyperref}
\usepackage{xurl}
\definecolor{accent}{HTML}{216875}
\definecolor{pale}{HTML}{EFF5F6}
\setlength{\emergencystretch}{3em}
\setlength{\parindent}{0pt}
\setlength{\parskip}{5pt}
\setlist{leftmargin=*,itemsep=3pt,topsep=4pt}
\newlength{\tableavail}
\renewcommand{\arraystretch}{1.13}
\lstset{basicstyle=\ttfamily\small,breaklines=true,columns=fullflexible,
  backgroundcolor=\color{pale},frame=single,framerule=0pt,xleftmargin=4pt,xrightmargin=4pt}
\pagestyle{fancy}\fancyhf{}
\lhead{\small LINKWISE / MODEM TESTING}
\rhead{\small LW-MODEM-KIWI-001 / Draft 0.6}
\cfoot{\small\thepage\ of \pageref{LastPage}}
\setcounter{tocdepth}{2}

% All figures, diagrams and tables are landscape and bounded in both dimensions.
\captionsetup{font=small,skip=12pt}
\newcommand{\landscapegraphic}[3]{%
 \begin{landscape}\thispagestyle{plain}\centering
 \includegraphics[width=0.96\linewidth,height=0.76\textheight,keepaspectratio]{#1}\par
 \captionof{figure}{#2}\label{#3}
 \end{landscape}
}
\newcommand{\landscapetikz}[3]{%
 \begin{landscape}\thispagestyle{plain}\centering
 \begin{adjustbox}{max totalsize={0.96\linewidth}{0.76\textheight},center}
 \input{#1}
 \end{adjustbox}\par
 \captionof{figure}{#2}\label{#3}
 \end{landscape}
}
\begin{document}
\begin{titlepage}
\centering
\vspace*{1in}
{\Huge\bfseries Linkwise Modem\par}
\vspace{0.2in}
{\LARGE KiwiSDR Network Test Plan\par}
\vspace{0.5in}
{\large One-way RF observation and decoder regression\par}
\vfill
\begin{minipage}{0.7\textwidth}
\textbf{Document:} LW-MODEM-KIWI-001\par
\textbf{Revision:} 0.6 (Draft)\par
\textbf{Date:} 9 November 2026\par
\textbf{Prepared for:} Linkwise project\par
\textbf{Test lead / operator:} Not assigned\par
\textbf{Execution status:} Not scheduled; prerequisites pending
\end{minipage}
\vspace{0.4in}
\begin{minipage}{0.88\textwidth}
This is a proposed experiment, not a test report. Eleven candidate receiver sites
are shortlisted; no recording windows are agreed and no RF tests have been
performed under this plan. Remote receive
observations do not establish bidirectional modem or agent-message delivery.
\end{minipage}
\vspace{0.5in}
\end{titlepage}
\begin{landscape}\thispagestyle{plain}\centering
\begin{adjustbox}{max totalsize={0.96\linewidth}{0.76\textheight},center}
\begin{tabular}{@{}lll@{}}
\toprule Revision & Date & Change\\\midrule
0.1 & 9 November 2026 & Initial KiwiSDR experiment draft\\
0.2 & 9 November 2026 & Template selection and objective/resource tailoring\\
0.3 & 9 November 2026 & Template-structured LaTeX edition and rendered diagram\\
0.4 & 9 November 2026 & Landscape figures/tables; PlantUML and TikZ views\\
0.5 & 9 November 2026 & Julian-centered GIS map and candidate register\\
0.6 & 9 November 2026 & Exact command sequence, expected results and bench helper\\\bottomrule
\end{tabular}
\end{adjustbox}
\captionof{table}{Document revision history}
\end{landscape}

\section*{Document control}
The Markdown draft is the maintained content source. This generated LaTeX edition
organizes the same technical material around the selected engineering Test Plan
template; it does not import the upstream acquisition document class or markings.
Template lineage is recorded in Appendix~\ref{app:tailoring}. This is a project
engineering draft, not a statement of compliance with an acquisition standard.

\begingroup\small
\tableofcontents
\endgroup
\clearpage
"""


def heading(title, label=None, level="section"):
    return "\\" + level + "{" + title + "}\n" + ("\\label{" + label + "}\n" if label else "")


def generate():
    source = SOURCE.read_text()
    pieces = re.split(r"^## (\d+)\. ([^\n]+)\n", source, flags=re.M)
    sections = {int(pieces[i]): (pieces[i+1], pieces[i+2]) for i in range(1, len(pieces), 3)}
    if set(sections) != set(range(1, 15)):
        raise ValueError("source section structure changed; update the tailoring map")
    tailoring, objectives = sections[13][1].split("### 13.1 Objective records and traceability\n")
    objectives, resource = objectives.split("### 13.2 Resources, readiness, and restoration\n")
    resource_table, readiness = resource.split("The readiness review must establish", 1)
    readiness = "The readiness review must establish" + readiness
    readiness, abort = readiness.split("Abort the affected block", 1)
    abort, companions = ("Abort the affected block" + abort).split("For execution, develop", 1)
    companions = "For execution, develop" + companions
    parts = [PREAMBLE]
    parts += [heading("Scope", "tp:scope"), render(sections[1][1]),
              heading("Implementation baseline and prerequisites", "md:2", "subsection"), render(sections[2][1])]
    parts += [heading("Referenced Documents", "tp:references"), render(sections[12][1])]
    parts += [heading("Test Items", "tp:items"), render("""Freeze these items in each as-tested manifest. Experimental builds have no qualified production baseline; deviations from a future production system are not yet applicable.

| Item | Configuration identification | Status |
| --- | --- | --- |
| Native modem | Git commit, build flags, waveform profile hash, frame format and expected bytes | Prototype under test |
| Transmit chain | Source WAV, playback/radio settings, monitor recording and station logs | Physical execution path requires qualification |
| Kiwi capture chain | Receiver endpoint/settings, client revision, sample continuity and timing evidence | Remote test instrumentation; not an ARQ peer |
| Import/comparison harness | Demodulator/resampler versions, transforms, fixed decode profiles and oracle | New tooling pending K0 |
""")]
    parts += [plant_figure("Kiwi_Physical_Setup",
        "Proposed station setup: separate audio, RF, local monitoring and exclusive PTT paths. The monitor path remains to be qualified.", "fig:physical-setup")]
    parts += [heading("Test Approach", "tp:approach")]
    for n in [3, 5, 6, 7, 8, 9]:
        body = render(sections[n][1])
        if n == 5:
            timing = tikz_figure("burst-timeline", "Illustrative timing: continuous pre-roll and post-roll, uniquely identified bursts and quiet guards. Spacing is schematic, not proportional to seconds.", "fig:burst-timeline")
            body = body.replace(r"\end{landscape}", r"\end{landscape}" + "\n" + timing, 1)
        section_heading = heading(escape(sections[n][0]), f"md:{n}", "subsection")
        if n == 8:
            body = body.replace(r"\begin{landscape}\thispagestyle{plain}\centering", r"\begin{landscape}\thispagestyle{plain}\centering" + "\n" + section_heading, 1)
            parts.append(body)
        else:
            parts += [section_heading, body]
        if n == 3:
            parts.append(DIAGRAM)
        elif n == 7:
            parts.append(plant_figure("Kiwi_Capture_Processing", "Proposed processing branches. Preserve originals and continuity evidence; score as-received and diagnostic outputs separately.", "fig:capture-processing"))
        elif n == 8:
            parts.append(plant_figure("Kiwi_Observation_Scoring", "Observation classification before scoring: confirmed transmission and eligible capture determine the denominator; a valid no-decode window remains a miss.", "fig:observation-scoring"))
            parts.append(tikz_figure("signal-annotation", "Conceptual annotation example, not measured RF data. Burst windows and noise references must be independently defined; visual energy alone is not exact recovery.", "fig:signal-annotation"))
        elif n == 9:
            parts.append(plant_figure("Kiwi_Qualification_Gates", "Proposed progression from tooling through pilot, replication, replay and optional diagnostics. These gates do not qualify a bidirectional operational link.", "fig:qualification-gates"))
    objective_body = render(objectives)
    lead, table_body = objective_body.split(r"\begin{landscape}", 1)
    table_body = table_body.replace(r"\thispagestyle{plain}\centering", r"\thispagestyle{plain}\centering" + "\n" + heading("Objective traceability", "tp:objectives", "subsection") + lead, 1)
    parts.append(r"\begin{landscape}" + table_body)
    parts += [heading("Resources, Facilities, and Equipment", "tp:resources"),
              heading("Receiver selection and recording profiles", "md:4", "subsection"), render(sections[4][1]),
              heading("Resource and personnel status", level="subsection"), render(resource_table)]
    parts += [heading("Safety", "tp:safety"), render(abort)]
    parts += [heading("Schedule", "tp:schedule"), render("""No calendar dates or receiver windows are committed. This schedule is dependency-based; each event starts only when its entry criteria are met. The pilot remains unscheduled.

| Event | Entry condition | Exit / next event |
| --- | --- | --- |
| Tooling preparation and K0 | Frozen stimuli and pinned capture/import tools | K0 tooling evidence accepted |
| Campaign readiness review | K0 complete; resources, windows, station operation and limits confirmed | Registered K1 window |
| K1 and K2 capture | Valid preflight, qualified station chain and readiness review | Pilot outcomes and anomaly records |
| K3 replication | K2 dataset and reviewed disposition | Three complete session blocks |
| K4 replay and report | Frozen holdout corpus; baseline/candidate builds | Paired results, exclusions and disposition |
| Optional K5 block | Separate variable matrix and readiness | Diagnostic report, no stronger qualification claim |
"""), heading("Readiness review", "tp:readiness", "subsection"), render(readiness)]
    parts += [heading("Data Management and Reporting", "md:10"), render(sections[10][1]), render(companions)]
    parts += [heading("Exact Test Sequence and Expected Results", "tp:procedure"), render(sections[14][1])]
    parts += ["\\appendix\n", heading("Implementation Backlog and Existing Commands", "tp:backlog"), render(sections[11][1]),
              heading("Template Lineage and Tailoring", "app:tailoring"), render("The crosswalk below refers to section numbers in the companion Markdown draft, not this edition.\n\n" + tailoring),
              r"\clearpage", heading("Campaign-readiness Checklist", "app:checklist"), render("""- Test lead and station operator assigned; no placeholder approval signatures treated as authorization.
- Matrix, waveform hashes, expected bytes, configuration and plan/procedure revisions frozen.
- K0 tooling evidence complete, including gap detection and exact-byte import checks.
- Receiver windows/settings/terms confirmed; station transmit and stop path qualified.
- Recording duration, airtime, resource limits, storage and restoration actions verified.
- Metric eligibility and exclusions declared; no decoding success used to hide capture failures.
- Report links each objective and verdict to as-tested configuration and raw/reduced evidence.

This is a planned readiness record. No item is represented as complete by inclusion in this checklist.
"""), "\\end{document}\n"]
    result = "\n".join(parts)
    # Body section references follow the maintained Markdown IDs; convert to LaTeX references.
    for phrase, replacement in {
        "section 9": r"Section~\ref{md:9}",
        "section 8": r"Section~\ref{md:8}",
        "section 10": r"Section~\ref{md:10}",
    }.items():
        result = result.replace(phrase, replacement)
    result = result.replace("sections 8 and 10", r"Sections~\ref{md:8} and~\ref{md:10}")
    TARGET.parent.mkdir(parents=True, exist_ok=True)
    TARGET.write_text(result)
    return TARGET


def build_figures(output):
    env = os.environ.copy()
    tool_root = ROOT / "docs/whitepaper/build/tools"
    java_home = env.get("LINKWISE_JAVA_HOME", env.get("JAVA_HOME"))
    if not java_home and (tool_root / "jre/bin/java").exists():
        java_home = str(tool_root / "jre")
    if java_home:
        env["JAVA_HOME"] = java_home
        env["PATH"] = str(Path(java_home) / "bin") + os.pathsep + env.get("PATH", "")
    env["JAVA_TOOL_OPTIONS"] = (env.get("JAVA_TOOL_OPTIONS", "") + " -Djava.awt.headless=true").strip()
    def tool(name, variable):
        requested = env.get(variable, name)
        located = shutil.which(requested, path=env.get("PATH"))
        if located:
            return located
        fallback = tool_root / "bin" / name
        if fallback.exists() and requested == name:
            return str(fallback)
        raise RuntimeError(f"Missing {name}; install it or set {variable}")
    plantuml = tool("plantuml", "LINKWISE_PLANTUML")
    tectonic = tool("tectonic", "LINKWISE_TECTONIC")
    diagrams = sorted((ROOT / "docs/testing/diagrams").glob("*.puml"))
    destination = output / "diagrams"
    destination.mkdir(parents=True, exist_ok=True)
    subprocess.run([plantuml, "--check-before-run", "--stop-on-error", "--latex",
                    "-Playout=smetana", "--output-dir", str(destination),
                    *map(str, diagrams)], env=env, check=True)
    for source in diagrams:
        name = re.search(r"^@startuml\s+(\S+)", source.read_text(), re.M).group(1)
        subprocess.run([tectonic, "--keep-logs", "--outdir", str(destination),
                        str(destination / (name + ".tex"))], env=env, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build", action="store_true", help="also build the PDF using latexmk/pdflatex")
    args = parser.parse_args()
    target = generate()
    print(target.relative_to(ROOT))
    if args.build:
        output = ROOT / "docs/build/kiwisdr-test-plan"
        output.mkdir(parents=True, exist_ok=True)
        build_figures(output)
        subprocess.run(["latexmk", "-pdf", "-interaction=nonstopmode", "-halt-on-error",
                        "-outdir=" + str(output), target.name], cwd=target.parent, check=True)
        # LaTeX includegraphics discards annotations from imported PDFs. Restore
        # receiver links on the map's retained vector text in the final edition.
        import fitz
        import json
        pdf_path = output / (target.stem + ".pdf")
        document = fitz.open(pdf_path)
        nodes = json.loads((ROOT / "docs/testing/data/kiwisdr-candidates.json").read_text())["candidates"]
        map_pages = [page for page in document if "CANDIDATE REGISTER" in page.get_text()]
        if len(map_pages) != 1:
            raise RuntimeError("Expected one candidate map page for link restoration")
        page = map_pages[0]
        receiver_urls = {node["url"] for node in nodes}
        for link in page.get_links():
            if link.get("uri") in receiver_urls:
                page.delete_link(link)
        for node in nodes:
            for text in [node["candidate_id"], node["label"]]:
                rectangles = page.search_for(text)
                if not rectangles:
                    raise RuntimeError("Map link label missing: " + text)
                for rectangle in rectangles:
                    page.insert_link({"kind": fitz.LINK_URI, "from": rectangle, "uri": node["url"]})
        temporary = pdf_path.with_suffix(".linked.pdf")
        document.save(temporary)
        document.close()
        temporary.replace(pdf_path)
        print(output / (target.stem + ".pdf"))


if __name__ == "__main__":
    main()
