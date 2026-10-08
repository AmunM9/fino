import { open } from "@tauri-apps/plugin-dialog";
import { useCallback } from "react";
import { errorMessage } from "../lib/ipc";
import { heicConversionAvailable } from "../lib/platform";
import { useApp } from "../state/AppProvider";

const PHOTO_EXTENSIONS = ["jpg", "jpeg", "jpe", "jfif"];
const HEIC_EXTENSIONS = ["heic", "heif", "hif"];

function asList(selection: string | string[] | null): string[] {
  if (!selection) return [];
  return Array.isArray(selection) ? selection : [selection];
}

/** Opens the native picker for photos (or a folder) and starts optimizing the choice. */
export function usePhotoPicker(): (directory: boolean) => Promise<void> {
  const { settings, start, notify } = useApp();
  const convertHeic = heicConversionAvailable && (settings?.heicToJpeg ?? false);

  return useCallback(
    async (directory: boolean) => {
      const extensions = convertHeic ? [...PHOTO_EXTENSIONS, ...HEIC_EXTENSIONS] : PHOTO_EXTENSIONS;
      try {
        const selection = await open({
          multiple: true,
          directory,
          filters: directory ? undefined : [{ name: "Fotos", extensions }],
        });
        start(asList(selection));
      } catch (e) {
        notify(errorMessage(e));
      }
    },
    [convertHeic, start, notify],
  );
}
