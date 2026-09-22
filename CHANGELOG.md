# Changelog

Toutes les modifications notables de ce projet sont documentées dans ce
fichier.

Le format suit [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/),
et ce projet adhère au [versionnage sémantique](https://semver.org/lang/fr/).

L'historique antérieur à ce fichier n'est pas reconstitué ici ; voir
l'historique Git pour les commits précédents.

## [0.3.0] - 2026-09-22

### ⚠️ Cassant

- `verify::verify_tag` renvoie désormais `Result<VerifyOutcome, VerifyError>`
  au lieu de `Result<Vec<VerificationReport>, VerifyError>`. Les rapports
  sont dans `VerifyOutcome::reports` (même contenu, même ordre qu'avant).

### Ajouté

- `verify::VerifyOutcome::album_search_error` : quand la recherche par
  album échoue (réseau, réponse illisible...) sans empêcher la
  vérification d'aboutir par ailleurs, l'erreur est maintenant conservée
  ici plutôt que silencieusement perdue. Le CLI l'affiche sur `stderr` en
  mode `--verbose`, à titre diagnostique, sans jamais bloquer l'affichage
  des résultats.
- `tempfile` déplacé en `[dev-dependencies]` dans `Cargo.toml` : il n'est
  utilisé que par les tests, pas par la bibliothèque elle-même.

### Modifié

- Passage en revue du projet par un outil d'analyse statique externe
  (Herald) et corrections associées :
  - `id3v1::TagFields` et `mpeg::vbr::XingFrameFields` : les fonctions de
    construction de données de test à 6 paramètres positionnels
    (`build_tag`, `xing_frame`) prennent maintenant un struct nommé.
  - `read_mp3_file_impl` (`lib.rs`) découpée en 5 fonctions nommées
    (`open_mp3_file`, `read_id3v2_tag`, `probe_audio_window`,
    `read_id3v1_tag_if_present`, `read_audio_if_requested`).
  - `VerificationTable::fmt` (`verify/table.rs`) découpée en 4 fonctions
    nommées (`build_rows`, `column_widths`, `write_row`/`write_separator`/
    `write_links`).
  - Les trois plus gros fichiers scindés en sous-modules, chacun sous 500
    lignes hors tests : `mpeg.rs` → `mpeg/mod.rs` + `mpeg/vbr.rs` ;
    `id3/frame.rs` → `id3/frame/mod.rs` + `id3/frame/decode.rs` ;
    `verify.rs` → `verify/mod.rs` + `verify/album.rs` + `verify/table.rs`.
  - Les blocs `#[cfg(test)] mod tests { ... }` des fichiers encore au-dessus
    de 500 lignes une fois cette limite mesurée hors code de test sortis
    dans un fichier `tests.rs` (ou `tests/`) sœur — même contenu, aucun
    changement de comportement (`decode.rs`, `frame/mod.rs`, `header.rs`
    — scindé en deux, `id3v1.rs`, `lib.rs`, `mpeg/vbr.rs`, `verify/mod.rs`).
- README : structure du projet reconstruite pour refléter les fichiers
  scindés ci-dessus, chemins `src/verify.rs`/`src/mpeg.rs` obsolètes
  corrigés vers `src/verify/`/`src/mpeg/vbr.rs`, mention du diagnostic
  `--verbose` ajouté.

### Corrigé

- `verify::album::search_by_album` était privée alors qu'appelée depuis
  le module parent (`verify::mod`) — passée en `pub(super)`. N'affectait
  que la compilation avec `cargo clippy`/une toolchain stricte, pas
  `cargo test`.

## [0.2.0] - 2026-09-21

### Ajouté

- Lecture du tag ID3v1 / ID3v1.1, situé dans les 128 derniers octets du
  fichier, indépendamment du tag ID3v2 (`Id3v1Tag`, `read_id3v1_tag`,
  nouveau module `src/id3v1.rs`).
- Affichage côte à côte des tags ID3v2 et ID3v1 dans le CLI lorsque les
  deux sont présents (`side_by_side` dans `main.rs`), avec les champs
  communs (`Title` à `Genre`) dans le même ordre des deux côtés pour
  qu'ils s'alignent ligne à ligne. Version et nombre de frames figurent
  désormais dans le titre de la section ID3v2 plutôt que sur des lignes
  séparées.
- Genres ID3v1 étendus : reconnaissance de l'extension Winamp (index 80 à
  191), en plus des 80 genres d'origine. L'affichage marque d'un `*` tout
  genre hors des 80 d'origine (nom Winamp ou index brut si vraiment
  inconnu), pour distinguer d'un coup d'œil un genre "officiel" ID3v1 d'un
  genre seulement conventionnel.
- Recherche MusicBrainz par album (`search_by_album` dans `src/verify.rs`),
  en complément de la recherche par morceau existante : pour un titre très
  repris en concert ou très réédité, où des centaines d'enregistrements
  distincts partagent le même score de pertinence maximal, chercher
  directement l'édition locale évite de la perdre dans la masse. Le
  rapport obtenu par cette voie, marqué `via_album` (colonne "Voie" dans
  le tableau du CLI), est placé en tête des résultats de `verify_tag`.
- Durée audio exacte à partir d'un en-tête Xing/Info ou VBRI trouvé dans
  la première frame (`AudioFormat::duration_is_exact`,
  `AudioFormat::average_bitrate_kbps`), plutôt qu'estimée à débit
  constant. L'affichage précède la durée d'un `~` quand elle n'est
  qu'une estimation, et montre le débit moyen réel plutôt que celui de
  la seule première frame quand il est connu.
- Prise en charge de l'unsynchronisation propre à une frame individuelle
  (ID3v2.4 uniquement, bit `n` des format flags), en plus de celle,
  déjà gérée, du tag entier.
- Tests unitaires pour `Mp3Error` (`Display` de chaque variante,
  `source()`), jusque-là dépourvu de toute couverture.
- Documentation manquante complétée sur l'ensemble du crate (52 éléments
  publics sans commentaire `///`, détectés via `#![warn(missing_docs)]`) :
  `Mp3Error` et ses variantes, les enums de `mpeg.rs`, plusieurs champs de
  `Id3v1Tag`/`Id3Version`/`Id3v2Tag`/`FrameContent`, et une doc de crate en
  tête de `lib.rs`.

### Modifié

- `Id3v2Tag::Display` réordonné (`Title`, `Artist`, `Album`, `Year`,
  `Comment`, `Track`, `Genre`, puis `Album Artist` et `Cover`, propres à
  ID3v2) pour correspondre à l'ordre de `Id3v1Tag::Display`.
- `Id3v1Tag::Display` affiche désormais toujours la ligne `Track` (avec
  `?` en son absence) plutôt que de l'omettre pour un tag ID3v1 classique,
  afin de ne pas décaler `Genre` par rapport à son vis-à-vis ID3v2.
- README mis à jour au fil des évolutions ci-dessus : structure du
  projet, table de l'API, exemples d'utilisation et de sortie du CLI
  (recopiés depuis une exécution réelle), section des fonctionnalités.

### Corrigé

- README : exemple d'utilisation et de sortie du CLI qui ne
  correspondaient plus à l'API actuelle (référençaient un ancien
  `mp3_file.header` et une fonction `read_frames` libre, tous deux
  disparus depuis) ; mentions obsolètes sur le type MIME des images
  `APIC` (en réalité déjà extrait) et le décodage des frames `COMM` (en
  réalité déjà correct) dans les limitations connues.
