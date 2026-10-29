/** Robot languages (`share/choregraphe/robotlanguages`, as `LANGUAGES` in choren-core). */
export const robotLanguages: Record<string, string> = {
  ar_SA: "Arabic", pt_BR: "Portuguese (Brazil)", zh_CN: "Chinese (Simplified)", cs_CZ: "Czech", da_DK: "Danish",
  nl_NL: "Dutch", en_US: "English", fi_FI: "Finnish", fr_FR: "French", de_DE: "German", el_GR: "Greek",
  it_IT: "Italian", ja_JP: "Japanese", ko_KR: "Korean", zh_TW: "Chinese (Traditional)", nn_NO: "Norwegian",
  pl_PL: "Polish", pt_PT: "Portuguese (Portugal)", ru_RU: "Russian", es_ES: "Spanish", sv_SE: "Swedish", tr_TR: "Turkish",
};

/** Codes sorted by their readable name. */
export const languageCodes = Object.keys(robotLanguages).sort((a, b) => robotLanguages[a].localeCompare(robotLanguages[b]));

/** "German" for `de_DE`; unknown codes stay as written. */
export const languageName = (code: string) => robotLanguages[code] ?? code;

/** The names ALTextToSpeech and the Set Language box use, by code (`templates::SPEECH_LANGUAGES`). */
const speechNames: Record<string, string> = {
  ar_SA: "Arabic", pt_BR: "Brazilian", zh_CN: "Chinese", cs_CZ: "Czech", da_DK: "Danish", nl_NL: "Dutch",
  en_US: "English", fi_FI: "Finnish", fr_FR: "French", de_DE: "German", el_GR: "Greek", it_IT: "Italian",
  ja_JP: "Japanese", ko_KR: "Korean", zh_TW: "MandarinTaiwan", nn_NO: "Norwegian", pl_PL: "Polish",
  pt_PT: "Portuguese", ru_RU: "Russian", es_ES: "Spanish", sv_SE: "Swedish", tr_TR: "Turkish",
};

/** Speech names sorted by their readable name. */
export const speechLanguages = languageCodes.map(code => speechNames[code]);

/** "Chinese (Traditional)" for `MandarinTaiwan`; unknown names stay as written. */
export const speechLanguageName = (name: string) => {
  const code = Object.keys(speechNames).find(c => speechNames[c] === name);
  return code ? languageName(code) : name;
};
