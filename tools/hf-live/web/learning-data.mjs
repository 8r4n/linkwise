export const BADGE_NAME = "HF Operator";

export const MISSION_CATALOG = Object.freeze({
  "read-burst": {
    title: "Read a burst",
    scenario: "usable-link",
    bandwidth: 500,
    action: "Send training burst",
    objective: "Compare the transmitted and received spectra of the same HF burst. A waterfall stacks successive spectra to show how the signal changes over time.",
    quiz: {
      question: "The waterfall shows spectra from successive times. What does its RX view show for this burst?",
      correct: "rx",
      choices: {
        tx: "The TX spectrum includes the simulated receiver's noise.",
        rx: "The RX spectrum shows the same burst after the simulated channel.",
        forecast: "The estimated path SNR is the spectrum of this received burst.",
      },
    },
  },
  "encrypted-delivery": {
    title: "Deliver an encrypted message",
    scenario: "usable-link",
    bandwidth: 2300,
    action: "Send encrypted training message",
    objective: "Send the preset chat through the modeled HF channel and wait for the recipient to authenticate it.",
  },
  "diagnose-link": {
    title: "Diagnose a failed link",
    scenario: "weak-link",
    bandwidth: 500,
    requires: "read-burst",
    action: "Send on the weak link",
    objective: "Follow a failed RF attempt, the station's next tuning decision, and the final delivery result.",
    quiz: {
      question: "What explains the final result on this link?",
      correct: "conditions",
      choices: {
        conditions: "The station adjusted its tuning after a failed burst, but this path still did not decode.",
        retuned: "The change of channel means the recipient authenticated the message.",
        energy: "The selected transmit power guarantees that each HF burst can be decoded.",
      },
    },
  },
  "compare-modes": {
    title: "Compare modem modes",
    scenario: "usable-link",
    bandwidth: 500,
    objective: "Send the same preset chat over the same path in BW500 and BW2300. Compare frame counts, total transmit airtime, retries, and outcomes.",
    quiz: {
      question: "Which difference does the modem mode explain directly?",
      correct: "capacity",
      choices: {
        capacity: "BW2300 carries more bytes per frame, so the same message can need fewer frames.",
        propagation: "Changing the modem mode changes the simulated weather along the path.",
        guarantee: "A wider mode guarantees delivery because each trial sends the same message.",
      },
    },
  },
});
