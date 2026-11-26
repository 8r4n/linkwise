import { BADGE_NAME, MISSION_CATALOG } from "./learning-data.mjs";
import {
  COMPARE_BANDWIDTHS, PROGRESS_EVENT, PROGRESS_STORAGE_KEY, createProgressStore,
  findTuningEvidence, hasBadge, isMissionComplete, isMissionUnlocked, isTrainingRequest,
  observedBurstKey, readTerminalEvidence,
} from "./learning-state.mjs";
import { getTrainingScenario } from "./training-scenarios.mjs";

const number = new Intl.NumberFormat("en-US", { maximumFractionDigits: 2 });

function node(tag, text) {
  const element = document.createElement(tag);
  if (text != null) element.textContent = text;
  return element;
}

export function createLearning({ id, launch, send, viewBurst, stationName }) {
  if (!Object.hasOwn(MISSION_CATALOG, id)) return null;
  const mission = MISSION_CATALOG[id];
  const scenario = getTrainingScenario(mission.scenario);
  const panel = document.getElementById("learning-panel");
  const title = document.getElementById("learning-title");
  const badge = document.getElementById("learning-badge");
  const objective = document.getElementById("learning-objective");
  const route = document.getElementById("learning-route");
  const progressLabel = document.getElementById("learning-progress");
  const prerequisite = document.getElementById("learning-prerequisite");
  const controls = document.getElementById("learning-actions");
  const details = document.getElementById("learning-evidence");
  const status = document.getElementById("learning-status");
  const view = document.getElementById("learning-signal");
  const quiz = document.getElementById("learning-quiz");
  const storageNotice = document.getElementById("learning-storage");
  const stationConsole = document.getElementById("station-console");
  const store = createProgressStore({ onChange: () => window.dispatchEvent(new Event(PROGRESS_EVENT)) });
  let ready = false;
  let pending = null;
  let expectedAttempt = null;
  let txSeen = null;
  let rxSeen = null;
  let diagnosis = null;

  const actions = id === "compare-modes"
    ? COMPARE_BANDWIDTHS.map(mode => [mode, "Run BW" + mode + " trial"])
    : [[mission.bandwidth, mission.action]];
  const buttons = actions.map(([mode, label]) => {
    const button = node("button", label);
    button.type = "button";
    button.addEventListener("click", () => begin(mode));
    controls.append(button);
    return button;
  });

  if (mission.quiz) {
    const group = node("fieldset");
    group.append(node("legend", mission.quiz.question));
    for (const [value, label] of Object.entries(mission.quiz.choices)) {
      const option = node("label");
      const input = node("input");
      input.type = "radio";
      input.name = "answer";
      input.value = value;
      input.required = true;
      option.append(input, node("span", label));
      group.append(option);
    }
    const check = node("button", "Check answer");
    check.type = "submit";
    quiz.append(group, check);
    quiz.addEventListener("submit", event => {
      event.preventDefault();
      const answer = new FormData(quiz).get("answer");
      if (!quizReady() || !isMissionUnlocked(store.progress, id)) return;
      if (answer !== mission.quiz.correct) {
        setStatus("That answer does not match the evidence. Review the result and try again.");
        return;
      }
      finish();
    });
  }

  panel.hidden = false;
  title.textContent = mission.title;
  objective.textContent = mission.objective;
  const reset = document.getElementById("learning-reset");
  reset.addEventListener("click", () => {
    store.reset();
    pending = null;
    expectedAttempt = txSeen = rxSeen = diagnosis = null;
    quiz.reset();
    setStatus("Learning progress cleared on this browser.");
    render();
    document.getElementById("learning-reset-options").open = false;
    progressLabel.focus();
  });
  window.addEventListener("storage", event => {
    if (event.key !== PROGRESS_STORAGE_KEY && event.key !== null) return;
    store.load();
    render();
  });
  render();

  function setStatus(message) {
    if (status.textContent !== message) status.textContent = message;
  }

  function trials() {
    return store.progress.compare.filter(trial => trial.scenarioId === scenario.id && trial.scenarioVersion === scenario.version);
  }

  function quizReady() {
    if (id === "read-burst") return txSeen !== null && txSeen === rxSeen;
    if (id === "diagnose-link") return diagnosis !== null;
    if (id === "compare-modes") return trials().length === COMPARE_BANDWIDTHS.length;
    return false;
  }

  function finish() {
    if (!store.complete(id)) return;
    setStatus(hasBadge(store.progress) ? "Mission complete. You earned the " + BADGE_NAME + " badge." : "Mission complete.");
    render();
    progressLabel.focus();
  }

  function begin(bandwidth) {
    if (!ready || pending || !isMissionUnlocked(store.progress, id) || isMissionComplete(store.progress, id)) return;
    if (!globalThis.crypto?.subtle) {
      setStatus("Open the simulation over HTTPS or localhost to run an encrypted training transfer.");
      return;
    }
    const request = {
      id: crypto.randomUUID(), from: scenario.source, to: scenario.target,
      kind: "chat", subject: "", body: scenario.message,
    };
    if (!isTrainingRequest(request, scenario)) return;
    pending = { request, bandwidth, sent: false };
    ready = false;
    expectedAttempt = txSeen = rxSeen = diagnosis = null;
    quiz.reset();
    setStatus("Preparing a fresh BW" + bandwidth + " training run…");
    render();
    launch(bandwidth);
  }

  function comparisonEvidence() {
    const cards = node("div");
    cards.className = "learning-trials";
    for (const bandwidth of COMPARE_BANDWIDTHS) {
      const card = node("section");
      card.className = "learning-trial";
      card.append(node("h3", "BW" + bandwidth));
      const trial = trials().find(item => item.bandwidthHz === bandwidth);
      if (trial) {
        const list = node("dl");
        for (const [label, value] of [
          ["Frames attempted / planned", trial.frameCount + " / " + trial.totalFrames],
          ["Total transmit airtime", number.format(trial.airtimeS) + " s"],
          ["Retries", trial.retries],
          ["Outcome", trial.outcome === "delivered" ? "Delivered and authenticated" : "Delivery failed"],
        ]) {
          const row = node("div");
          row.append(node("dt", label), node("dd", String(value)));
          list.append(row);
        }
        card.append(list);
      } else {
        card.append(node("p", "Trial pending."));
      }
      cards.append(card);
    }
    return cards;
  }

  function diagnosisEvidence() {
    const list = node("ul");
    const { failure, tuning } = diagnosis;
    list.append(node("li", "Attempt #" + failure.attemptId + " failed on " + failure.band
      + " at " + number.format(failure.powerW) + " W (" + number.format(failure.measuredSinrDb) + " dB SINR)."));
    list.append(node("li", "Attempt #" + tuning.attemptId + " used " + tuning.band
      + " at " + number.format(tuning.powerW) + " W after the failure."));
    list.append(node("li", "Final result: the recipient did not receive the message."));
    return list;
  }

  function render() {
    const progress = store.progress;
    const complete = isMissionComplete(progress, id);
    const unlocked = isMissionUnlocked(progress, id);
    progressLabel.textContent = (complete ? "Complete · " : "") + Object.keys(progress.completed).length
      + " of " + Object.keys(MISSION_CATALOG).length + " missions complete";
    badge.hidden = !hasBadge(progress);
    badge.textContent = BADGE_NAME;
    prerequisite.hidden = unlocked;
    stationConsole.toggleAttribute("data-learning-locked", !unlocked);
    const prerequisiteLink = prerequisite.querySelector("a");
    if (!unlocked) {
      prerequisiteLink.href = "?mission=" + mission.requires;
      prerequisiteLink.textContent = MISSION_CATALOG[mission.requires].title;
    }
    route.hidden = !unlocked;
    route.textContent = stationName(scenario.source) + " → " + stationName(scenario.target)
      + ' · Preset chat “' + scenario.message + '”';
    controls.hidden = !unlocked || complete;
    for (const button of buttons) button.disabled = !ready || pending !== null;
    view.hidden = id !== "read-burst" || expectedAttempt === null || !unlocked;
    quiz.hidden = !unlocked || !quizReady() || complete || (id === "compare-modes" && pending !== null);
    storageNotice.hidden = store.canSave;
    details.hidden = !unlocked;
    if (id === "compare-modes") {
      details.replaceChildren(comparisonEvidence());
    } else if (diagnosis) {
      details.replaceChildren(diagnosisEvidence());
    } else {
      details.replaceChildren();
    }
  }

  return {
    get scenarioId() { return scenario.id; },
    get initialBandwidth() {
      if (id !== "compare-modes") return mission.bandwidth;
      return COMPARE_BANDWIDTHS.find(mode => !trials().some(trial => trial.bandwidthHz === mode)) ?? mission.bandwidth;
    },
    onSnapshot(value) {
      const matching = value.scenario?.id === scenario.id && value.scenario.version === scenario.version;
      ready = true;
      if (pending && !pending.sent) {
        if (!matching || value.bandwidth_hz !== pending.bandwidth) {
          this.onError("The expected training conditions were unavailable. Start a fresh trial.");
          return;
        }
        pending.sent = true;
        try {
          send(pending.request);
          setStatus("Encrypted training transfer submitted. Waiting for the actual RF result.");
        } catch (error) {
          this.onError(error.message);
        }
      } else if (matching && !status.textContent) {
        setStatus("Training conditions ready.");
      }
      render();
    },
    onError(message) {
      pending = null;
      ready = true;
      setStatus(message);
      render();
    },
    onRadio(value) {
      if (value.id !== pending?.request.id) return;
      if (id === "read-burst" && expectedAttempt === null && value.state === "transmitting" && value.attempt_id != null) {
        expectedAttempt = value.attempt_id;
        viewBurst(scenario.source, expectedAttempt);
        setStatus("Inspect the training burst in the signal panel. Compare TX with Received (RX).");
        render();
      } else if (id === "diagnose-link" && ["transmitting", "failed"].includes(value.state)) {
        const state = value.state === "failed" ? "Attempt failed" : "Tuning";
        setStatus(state + " · " + (value.band ?? "") + " · " + value.power_w + " W");
      }
    },
    onCapture({ attempt, stage }) {
      if (id !== "read-burst" || isMissionComplete(store.progress, id)) return;
      const key = observedBurstKey(attempt, scenario, expectedAttempt);
      if (!key) return;
      if (stage === "tx") txSeen = key;
      if (stage === "rx" && txSeen === key) rxSeen = key;
      if (quizReady()) {
        setStatus("The TX and RX spectra for the same burst have been displayed. Answer the question to complete the mission.");
        render();
      } else if (stage === "tx") {
        setStatus("TX spectrum displayed. Switch to Received (RX) to see the same burst after the channel.");
      }
    },
    onMessage(event) {
      if (!pending || event.id !== pending.request.id || !["delivered", "failed"].includes(event.state)) return;
      const evidence = readTerminalEvidence(event, pending.request, scenario, pending.bandwidth);
      pending = null;
      if (id === "read-burst" && expectedAttempt !== null) {
        render();
        return;
      }
      if (!evidence) {
        setStatus("No verified radio evidence was available for this run. Start a fresh trial.");
      } else if (id === "encrypted-delivery" && evidence.outcome === "delivered") {
        finish();
        return;
      } else if (id === "diagnose-link") {
        diagnosis = findTuningEvidence(evidence);
        setStatus(diagnosis ? "The receiver failed after a verified tuning change. Review both attempts and answer the question."
          : "This run did not show both a failed decode and a later tuning change. Start a fresh trial.");
      } else if (id === "compare-modes" && store.recordTrial(evidence)) {
        const outcome = evidence.outcome === "delivered" ? "delivered and authenticated" : "failed";
        setStatus("BW" + evidence.bandwidthHz + " trial " + outcome + ". "
          + (quizReady() ? "Compare both results and answer the question." : "Run the other modem mode."));
      } else {
        setStatus("The transfer did not arrive and authenticate. Start a fresh trial.");
      }
      render();
    },
  };
}
