import type { Asset } from "./core/model";
import { fromSeconds } from "./core/time";

/** Probe a user file in the browser and describe it as an Asset. */
export function probeFile(file: File): Promise<Asset> {
  const url = URL.createObjectURL(file);
  const id = `asset-${crypto.randomUUID()}`;
  const type = file.type.split("/")[0];

  if (type === "image") {
    return new Promise((resolve, reject) => {
      const img = new Image();
      img.onload = () => resolve({ id, name: file.name, kind: "image", duration: 0, width: img.naturalWidth, height: img.naturalHeight, url });
      img.onerror = () => reject(new Error(`Cannot read image ${file.name}`));
      img.src = url;
    });
  }
  if (type !== "video" && type !== "audio") return Promise.reject(new Error(`Unsupported file type: ${file.type || file.name}`));

  return new Promise((resolve, reject) => {
    const el = document.createElement(type);
    el.preload = "metadata";
    el.onloadedmetadata = () => {
      const v = el as HTMLVideoElement;
      resolve({
        id, name: file.name, kind: type, url, duration: fromSeconds(el.duration),
        width: type === "video" ? v.videoWidth : undefined, height: type === "video" ? v.videoHeight : undefined,
      });
    };
    el.onerror = () => reject(new Error(`Cannot decode ${file.name}`));
    el.src = url;
  });
}
