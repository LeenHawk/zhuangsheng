import { readFile, writeFile } from "node:fs/promises";

const gradlePath =
  process.argv[2] ??
  "apps/desktop/src-tauri/gen/android/app/build.gradle.kts";
const releaseSigningLine =
  '            signingConfig = signingConfigs.getByName("release")';

let source = await readFile(gradlePath, "utf8");

if (source.includes(releaseSigningLine)) {
  process.stdout.write(`Android signing is already configured in ${gradlePath}\n`);
  process.exit(0);
}

const androidMarker = "\nandroid {\n";
const releaseMarker =
  '        getByName("release") {\n            isMinifyEnabled';

if (!source.includes(androidMarker) || !source.includes(releaseMarker)) {
  throw new Error(
    `Unsupported generated Gradle template in ${gradlePath}; signing was not changed`,
  );
}

const signingEnvironment = `
fun requiredSigningEnv(name: String): String =
    System.getenv(name) ?: error("Missing required environment variable: $name")

val releaseKeystorePath = requiredSigningEnv("ANDROID_KEYSTORE_PATH")
val releaseKeystorePassword = requiredSigningEnv("ANDROID_KEYSTORE_PASSWORD")
val releaseKeyAlias = requiredSigningEnv("ANDROID_KEY_ALIAS")
val releaseKeyPassword = requiredSigningEnv("ANDROID_KEY_PASSWORD")
`;

const signingConfig = `
    signingConfigs {
        create("release") {
            storeFile = file(releaseKeystorePath)
            storePassword = releaseKeystorePassword
            keyAlias = releaseKeyAlias
            keyPassword = releaseKeyPassword
        }
    }
`;

source = source.replace(
  androidMarker,
  `${signingEnvironment}\nandroid {${signingConfig}`,
);
source = source.replace(
  releaseMarker,
  `        getByName("release") {\n${releaseSigningLine}\n            isMinifyEnabled`,
);

await writeFile(gradlePath, source);
process.stdout.write(`Configured Android release signing in ${gradlePath}\n`);
