import { messageLimit, textBytes } from "./station-messaging.js";

export function createStationConsole({ send }) {
  const root = document.getElementById("station-console");
  if (!root) return { configure() {}, receive() {}, receiveRadio() {}, selectStation() {} };
  const stylesheet = document.createElement("link");
  stylesheet.rel = "stylesheet";
  stylesheet.href = new URL("./station-console.css", import.meta.url);
  document.head.append(stylesheet);
  root.innerHTML = `
    <details>
      <summary>Station console</summary>
      <div class="station-console-grid">
        <form class="station-compose" aria-label="Send an encrypted message">
          <p>Send encrypted chat or email to a simulated station. Your message is split into HF frames and authenticated at the destination.</p>
          <div class="station-endpoints">
            <label>From <select name="from" required></select></label>
            <label>To <select name="to" required></select></label>
          </div>
          <label>Message type <select name="kind"><option value="chat">Chat</option><option value="email">Email</option></select></label>
          <label class="station-subject" hidden>Subject <input name="subject" maxlength="48"></label>
          <label>Message <textarea name="body" rows="3" maxlength="240" required></textarea></label>
          <p class="station-budget"></p>
          <button type="submit">Send encrypted</button>
          <p class="station-feedback" role="status"></p>
        </form>
        <div class="station-inbox">
          <h3>Transfers and inbox</h3>
          <p class="station-radio" role="status" hidden></p>
          <p class="station-empty">Delivered messages appear here.</p>
          <ol class="station-transfers" aria-live="polite"></ol>
        </div>
      </div>
    </details>`;
  const form = root.querySelector("form");
  const { from, to, kind, subject, body } = form.elements;
  const subjectField = root.querySelector(".station-subject");
  const budget = root.querySelector(".station-budget");
  const feedback = root.querySelector(".station-feedback");
  const transfers = root.querySelector(".station-transfers");
  const empty = root.querySelector(".station-empty");
  const radio = root.querySelector(".station-radio");
  const stationIds = new Map();
  const cards = new Map();
  let limit = messageLimit(9);

  const stationLabel = id => {
    const station = stationIds.get(id);
    return station ? `${station.name} (${id})` : id;
  };
  const counter = () => {
    const used = textBytes(body.value) + (kind.value === "email" ? textBytes(subject.value) : 0);
    budget.textContent = `${used} / ${limit} UTF-8 bytes on this mode`;
    return used;
  };

  function destinations(previous) {
    const source = stationIds.get(from.value);
    const options = [...stationIds.values()].filter(station => station.id !== from.value);
    to.replaceChildren(...options.map(station => new Option(stationLabel(station.id), station.id)));
    if (options.some(station => station.id === previous)) {
      to.value = previous;
    } else if (source && options.length) {
      const distance = station => {
        const theta = (station.lat - source.lat) * Math.PI / 180;
        const phi = (station.lon - source.lon) * Math.PI / 180;
        return Math.sin(theta / 2) ** 2 + Math.sin(phi / 2) ** 2
          * Math.cos(station.lat * Math.PI / 180) * Math.cos(source.lat * Math.PI / 180);
      };
      to.value = options.reduce((best, station) => distance(station) < distance(best) ? station : best).id;
    }
  }

  from.addEventListener("change", () => destinations(to.value));
  kind.addEventListener("change", () => {
    const email = kind.value === "email";
    subjectField.hidden = !email;
    subject.required = email;
    counter();
  });
  form.addEventListener("input", counter);
  form.addEventListener("submit", event => {
    event.preventDefault();
    if (!globalThis.crypto?.subtle) {
      feedback.textContent = "Open this page over HTTPS to send encrypted messages.";
      return;
    }
    if (counter() > limit || (kind.value === "email" && textBytes(subject.value) > 48)) {
      feedback.textContent = "Shorten the message or choose a wider mode.";
      return;
    }
    const request = { id: crypto.randomUUID(), from: from.value, to: to.value, kind: kind.value,
      subject: subject.value, body: body.value };
    if (!request.body.trim()) {
      feedback.textContent = "Enter a message.";
      return;
    }
    send(request);
    body.value = "";
    subject.value = "";
    counter();
    feedback.textContent = "Message queued for encryption.";
  });

  return {
    configure(stations, mode) {
      const capacity = /2300|2750/.test(String(mode)) || mode === 86 ? 86 : 9;
      limit = messageLimit(capacity);
      const original = from.value;
      const recipient = to.value;
      stationIds.clear();
      for (const station of stations) stationIds.set(station.id, station);
      from.replaceChildren(...stations.map(station => new Option(stationLabel(station.id), station.id)));
      if (stationIds.has(original)) from.value = original;
      destinations(recipient);
      root.hidden = stations.length < 2;
      if (!stations.length) radio.hidden = true;
      counter();
    },
    selectStation(id) {
      if (stationIds.has(id)) {
        from.value = id;
        destinations(to.value);
      }
    },
    receiveRadio(data) {
      if (!data) return;
      const power = data.power_w ?? data.powerW;
      const frequency = data.frequency_mhz;
      const want = data.band ? [data.band, Number.isFinite(frequency) ? `${frequency} MHz` : ""].filter(Boolean).join(" · ") : "";
      const measured = Number.isFinite(data.measured_sinr_db) ? `${data.measured_sinr_db.toFixed(1)} dB SINR` : "";
      const parts = [
        data.from ? stationLabel(data.from) : "Station radio",
        data.frameNumber && data.attempt ? `Frame ${data.frameNumber}, attempt ${data.attempt}` : "",
        want,
        Number.isFinite(power) ? `${power} W` : "",
        measured,
        data.reason,
      ].filter(Boolean);
      radio.textContent = parts.join(" · ");
      radio.hidden = false;
    },
    receive(data) {
      if (!data?.id) return;
      empty.hidden = true;
      let card = cards.get(data.id);
      if (!card) {
        if (cards.size === 16) {
          const oldest = cards.keys().next().value;
          cards.get(oldest).remove();
          cards.delete(oldest);
        }
        card = document.createElement("li");
        cards.set(data.id, card);
        transfers.prepend(card);
      }
      const route = document.createElement("strong");
      route.textContent = `${data.kind === "email" ? "Email" : "Chat"} from ${stationLabel(data.from)} to ${stationLabel(data.to)}`;
      const state = document.createElement("p");
      if (data.state === "delivered") state.textContent = `Delivered and authenticated · ${data.encryption}`;
      else if (data.state === "sending") state.textContent = `${data.receivedFrames} of ${data.totalFrames} HF frames received`;
      else if (data.state === "failed") state.textContent = data.error || "Delivery failed.";
      else state.textContent = "Encrypting";
      card.dataset.state = data.state;
      card.replaceChildren(route, state);
      if (data.state === "delivered") {
        if (data.subject) {
          const heading = document.createElement("strong");
          heading.textContent = data.subject;
          card.append(heading);
        }
        const message = document.createElement("p");
        message.className = "station-delivered";
        message.textContent = data.body;
        card.append(message);
      }
    },
  };
}
