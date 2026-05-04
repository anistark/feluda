# Java Example

Demonstrates Feluda license scanning for Java projects using Maven (`pom.xml`) and Gradle (`build.gradle`).

## Maven Example

```bash
feluda --path examples/java-example/maven-example
```

## Gradle Example

```bash
feluda --path examples/java-example/gradle-example
```

## What Feluda scans

- **Maven**: parses `pom.xml` — `<dependencies>`, `<dependencyManagement>`, and `<properties>` for version resolution
- **Gradle**: parses `build.gradle` / `build.gradle.kts` — `implementation`, `api`, `compileOnly`, `runtimeOnly`, `annotationProcessor` configurations

Test-scoped dependencies (`scope=test` in Maven, `testImplementation` in Gradle) are excluded.

License data is fetched from Maven Central.
