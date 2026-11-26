import { isMissionComplete, PROGRESS_EVENT, PROGRESS_STORAGE_KEY, readProgress } from "./learning-state.mjs";
import {
  createLabProgressStore, LAB_EXERCISES, LAB_PREREQUISITE, LAB_PROGRESS_EVENT, LAB_PROGRESS_KEY,
  comparisonReady, labTrial, readTrialEvidence,
} from "./antenna-lab-state.mjs";

const number = new Intl.NumberFormat("en-US", { maximumFractionDigits: 2 });

const exercises = {
  tx: {
    title: "Compare transmit antennas and bearings",
    instructions: "Send the same frame with a dipole, an aligned Yagi, and a Yagi pointed 90° away from the path bearing. Compare the transmitter’s conducted watts with its equivalent isotropic radiated power (EIRP) toward the receiver. The receive setup stays fixed.",
    question: "What changed the EIRP toward the receiving station?",
    answers: [
      ["watts", "The transmitter supplied more conducted watts when the antenna pointed toward the receiver."],
      ["pattern", "The transmitting antenna put more of the same conducted power toward the receiving station."],
      ["receiver", "The receiving antenna changed the EIRP of the transmitting station."],
    ],
    answer: "pattern",
    success: "At the same conducted power, the antenna pattern and bearing changed how much power the transmitter radiated toward the receiver.",
  },
  rx: {
    title: "Switch receive ports",
    instructions: "Repeat the same transmission with a receive dipole and an active receiving loop. Compare what reaches the decoder while the transmitter keeps the same power, antenna, and bearing.",
    question: "What explains the change in measured SINR?",
    answers: [
      ["transmitter", "The transmitter increased its EIRP between the two runs."],
      ["decoder", "A strong transmitted spectrum always produces a decoded frame."],
      ["receiver", "The receive port changed the signal reaching the decoder relative to the same background noise."],
    ],
    answer: "receiver",
    success: "With the same transmit EIRP, changing the receive setup changed the SINR at the decoder.",
  },
};

function element(tag, content, className) {
  const node = document.createElement(tag);
  if (content != null) node.textContent = content;
  if (className) node.className = className;
  return node;
}

function action(label) {
  const button = element("button", label);
  button.type = "button";
  return button;
}

function link(label, href) {
  const node = element("a", label, "antenna-lab-link");
  node.href = href;
  return node;
}

function unit(value, suffix) {
  return Number.isFinite(value) ? number.format(value) + suffix : "Unavailable";
}

export function mountAntennaLab({ root, transport, onCapture, learningHref = "../learn/" }) {
  if (!root) return null;

  const store = createLabProgressStore({ onChange: () => window.dispatchEvent(new Event(LAB_PROGRESS_EVENT)) });
  const results = new Map();
  let lastAttemptId = 0;
  let fixture = null;
  let pending = null;
  let nextTrial = 0;
  let unlocked = false;
  let originError = false;

  root.classList.add("panel", "antenna-lab");
  const heading = element("div", null, "antenna-lab-heading");
  const title = element("h2", "Antenna Lab");
  title.id = "antenna-lab-heading";
  const progress = element("p", null, "antenna-lab-muted");
  progress.dataset.labProgress = "";
  progress.tabIndex = -1;
  heading.append(title, progress);
  const intro = element("p", "Compare actual BW500 decoder results on the same simulated 20 m link. Each trial resets the channel conditions so you can isolate the antenna change. Bearings mark each modeled antenna’s direction of maximum gain.");
  const requirement = element("p");
  requirement.dataset.labPrerequisite = "";
  requirement.append("Complete ", link("Read a burst", "./?mission=" + LAB_PREREQUISITE), " to unlock these exercises.");
  const route = element("p", null, "antenna-lab-muted");
  const status = element("p", null, "antenna-lab-status");
  status.dataset.labStatus = "";
  status.setAttribute("role", "status");
  const content = element("div", null, "antenna-lab-exercises");
  const storageNotice = element("p", "Progress is available for this visit. Browser storage is unavailable.", "antenna-lab-muted");
  const footer = element("footer", null, "antenna-lab-footer");
  const resetOptions = element("details");
  const reset = action("Clear lab progress");
  reset.dataset.labReset = "";
  resetOptions.append(element("summary", "Reset Antenna Lab"), element("p", "Clears lab completion and removes trial results from this panel. Your other Learn progress stays saved."), reset);
  const reload = link("Reload lab", "./?lab=antenna");
  reload.hidden = true;
  footer.append(link("Back to Learn", learningHref), link("Free simulation", "./"), reload, resetOptions);
  root.replaceChildren(heading, intro, requirement, route, status, content, storageNotice, footer);

  const panels = Object.fromEntries(LAB_EXERCISES.map(id => [id, buildExercise(id)]));
  setStatus("Preparing the matched 20 m link…");
  refresh();
  root.hidden = false;

  reset.addEventListener("click", () => {
    if (pending) return;
    results.clear();
    store.reset();
    for (const panel of Object.values(panels)) {
      panel.form.reset();
      for (const card of Object.values(panel.cards)) card.output.replaceChildren();
    }
    resetOptions.open = false;
    setStatus("Antenna Lab progress cleared.");
    render();
    progress.focus();
  });

  window.addEventListener(PROGRESS_EVENT, refresh);
  window.addEventListener("pageshow", refresh);
  window.addEventListener("storage", event => {
    if (event.key === LAB_PROGRESS_KEY || event.key === PROGRESS_STORAGE_KEY || event.key === null) refresh();
  });

  function setStatus(message) {
    if (status.textContent !== message) status.textContent = message;
  }

  function refresh() {
    store.reload();
    unlocked = isMissionComplete(readProgress(), LAB_PREREQUISITE);
    render();
  }

  function buildExercise(id) {
    const spec = exercises[id];
    const section = element("section", null, "antenna-lab-exercise");
    section.dataset.labExercise = id;
    const subtitle = element("h3", spec.title);
    subtitle.id = "antenna-lab-exercise-" + id;
    section.setAttribute("aria-labelledby", subtitle.id);
    const instructions = element("p", spec.instructions);
    const grid = element("div", null, "antenna-lab-trials");
    const cards = Object.fromEntries(labTrial[id].map(spec => {
      const card = element("article", null, "antenna-lab-trial");
      const label = element("h4", spec.label);
      const button = action(spec.button);
      button.dataset.labTrial = spec.id;
      const output = element("div", null, "antenna-lab-result");
      output.dataset.labEvidence = spec.id;
      button.addEventListener("click", () => begin(id, spec));
      card.append(label, button, output);
      grid.append(card);
      return [spec.id, { button, output }];
    }));
    const form = element("form", null, "antenna-lab-question");
    const group = element("fieldset");
    group.append(element("legend", spec.question));
    for (const [value, text] of spec.answers) {
      const label = element("label");
      const answer = element("input");
      answer.name = "antenna-answer";
      answer.type = "radio";
      answer.value = value;
      answer.required = true;
      label.append(answer, element("span", text));
      group.append(label);
    }
    const submit = action("Check answer");
    submit.type = "submit";
    form.append(group, submit);
    form.addEventListener("submit", event => {
      event.preventDefault();
      if (!unlocked || pending || !comparisonReady(id, results)) return;
      if (new FormData(form).get("antenna-answer") !== spec.answer) {
        setStatus("Compare the measured results and try again.");
        return;
      }
      store.complete(id);
      setStatus(spec.success);
      render();
      progress.focus();
    });

    const completed = element("p", "Exercise complete", "antenna-lab-complete");
    completed.dataset.labCompleted = id;
    section.append(subtitle, instructions, grid, form, completed);
    content.append(section);
    return { form, cards, completed };
  }

  function begin(exercise, spec) {
    if (!fixture || pending || !unlocked || originError) return;
    const trial = { id: "antenna-" + ++nextTrial, ...spec.request(fixture) };
    const currentTransport = transport();
    if (!currentTransport?.runAntennaTrial) {
      setStatus("The simulation is unavailable. Reload the lab and try again.");
      reload.hidden = false;
      return;
    }
    pending = { id: trial.id, exercise, spec, trial };
    setStatus("Preparing " + spec.label.toLowerCase() + " trial…");
    render();
    try {
      currentTransport.runAntennaTrial(trial);
    } catch (error) {
      pending = null;
      setStatus(error.message);
      render();
    }
  }

  function display(result, card) {
    const list = element("dl");
    for (const [label, value] of [
      ["Conducted power", unit(result.powerW, " W")],
      ["TX bearing", unit(result.txAzimuth, "°")],
      ["TX EIRP toward receiver", unit(result.eirpDbw, " dBW")],
      ["RX setup", result.rxLabel + " at " + unit(result.rxAzimuth, "°")],
      ["Measured SNR", unit(result.snrDb, " dB")],
      ["Measured SINR", unit(result.sinrDb, " dB")],
      ["Frame result", result.state === "delivered" ? "Decoded and bytes verified" : "Frame failed to decode"],
    ]) {
      const row = element("div");
      row.append(element("dt", label), element("dd", value));
      list.append(row);
    }
    const view = link("Inspect received burst #" + result.attemptId, "#waveform-title");
    view.dataset.labCapture = String(result.attemptId);
    view.addEventListener("click", event => {
      event.preventDefault();
      onCapture(result.source, result.attemptId);
    });
    card.output.replaceChildren(list, view);
  }

  function render() {
    const complete = LAB_EXERCISES.filter(id => store.progress.completed[id]).length;
    progress.textContent = complete + " of " + LAB_EXERCISES.length + " exercises complete";
    requirement.hidden = unlocked;
    route.hidden = !unlocked || !fixture;
    status.hidden = !unlocked;
    content.hidden = !unlocked;
    storageNotice.hidden = !unlocked || store.canSave;
    resetOptions.hidden = !unlocked;
    reset.disabled = pending !== null;
    for (const [id, panel] of Object.entries(panels)) {
      const done = store.progress.completed[id];
      panel.form.hidden = !unlocked || done || pending !== null || !comparisonReady(id, results);
      panel.completed.hidden = !done;
      for (const [trial, { button }] of Object.entries(panel.cards)) {
        button.disabled = !unlocked || !fixture || pending !== null || originError || results.has(trial);
      }
    }
  }

  return {
    onSnapshot(value) {
      const next = value.antennaLab;
      if (next?.id !== "antenna-lab" || next.band !== "20m" || !(next.powerW > 0)
        || !Number.isFinite(next.txBearingDeg) || !Number.isFinite(next.rxBearingDeg)) return;
      fixture = next;
      const stationName = id => value.stations?.find(station => station.id === id)?.name ?? id;
      route.textContent = stationName(fixture.source) + " → " + stationName(fixture.target)
        + " · BW500 · " + unit(fixture.powerW, " W") + " conducted";
      setStatus("Choose a trial to send the matched test frame.");
      render();
    },

    onTrial(value) {
      if (!pending || value?.id !== pending.id) return;
      const { exercise, spec, trial } = pending;
      if (value.state === "rotating" || value.state === "transmitting") {
        setStatus(value.state === "rotating" ? "Moving antennas to their trial bearings…" : "Transmitting the matched BW500 frame…");
        return;
      }

      pending = null;
      const result = readTrialEvidence(value, fixture, trial);
      const matched = result && result.attemptId > lastAttemptId;
      if (matched) {
        lastAttemptId = result.attemptId;
        results.set(spec.id, result);
        display(result, panels[exercise].cards[spec.id]);
        if (comparisonReady(exercise, results)) {
          setStatus("The matched results are ready. Review the captures or answer the question.");
        } else if (labTrial[exercise].every(trial => results.has(trial.id))) {
          setStatus("The recorded trials did not form a matched comparison. Reload the lab to try again.");
          reload.hidden = false;
        } else {
          setStatus(result.state === "failed" ? "The received frame failed to decode. Its capture counts as trial evidence."
            : "The receiver decoded and verified the frame.");
        }
      } else {
        setStatus(value.state === "error" && value.error ? value.error : "This result did not match the trial. Run it again.");
      }
      render();
      if (matched) panels[exercise].cards[spec.id].output.querySelector("[data-lab-capture]").focus();
    },

    onError(message) {
      pending = null;
      originError = true;
      reload.hidden = false;
      setStatus(typeof message === "string" && message ? message : "Unable to run the matched antenna trials.");
      render();
    },
  };
}
