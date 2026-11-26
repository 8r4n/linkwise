import { MISSION_CATALOG } from "../../tools/hf-live/web/learning-data.mjs";

export const learningDescription = "Follow a signal from audio samples to a decoded message in four hands-on HF missions. See how channel conditions and station choices affect the outcome.";

const descriptions = {
  "read-burst": "Follow one transmission from sender to receiver. Compare its transmitted and received spectra and check the decode result.",
  "encrypted-delivery": "Send a sample chat message between simulated stations. Follow its frames over the air and verify authentication at the recipient.",
  "diagnose-link": "Read a failed decode and the receiver’s signal measurements. Follow what the station changes and check the final delivery result.",
  "compare-modes": "Send the same message over the same link with BW500 and BW2300. Compare the delivery outcomes, frame counts, retries, and simulated airtime.",
};
export const learningMissions = Object.entries(MISSION_CATALOG).map(([id, mission]) => ({
  id,
  title: mission.title,
  description: descriptions[id],
  prerequisite: MISSION_CATALOG[mission.requires]?.title,
}));

export const learningLayers = [
  {
    title: "Waveform and modem",
    description: "The encoder turns frame bytes into audio samples. The channel changes those samples in transit, and the receiver tries to recover the frame and validate its checksum.",
  },
  {
    title: "Station decisions",
    description: "Each station queues traffic, chooses when and where to transmit, and can retry after a failed decode. Those choices affect whether the message reaches its destination.",
  },
  {
    title: "Messaging",
    description: "The messaging layer prepares and encrypts chat or email, then reassembles the received frames. The recipient verifies the message after decryption.",
  },
];
