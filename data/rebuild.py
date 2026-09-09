#!/usr/bin/env python3
"""Offline, deterministic bundle assembly. See docs/content.md for source acquisition.

This developer utility never runs during import, launch, or compilation. It reads
only the explicitly supplied directory and verifies every source hash first.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import unicodedata

ROOT = Path(__file__).resolve().parent

CURATED = {
    "french": """le la les un une des du de à et ou mais donc car ni que qui quoi où quand comment pourquoi je tu il elle nous vous ils elles me te se mon ton son ma ta sa mes tes ses notre votre leur nos vos leurs ce cet cette ces être avoir faire dire aller voir savoir pouvoir vouloir venir devoir prendre donner parler aimer penser trouver laisser demander comprendre mettre rester passer porter regarder appeler jouer vivre croire entendre suivre attendre sentir sortir entrer lire écrire manger boire dormir marcher courir ouvrir fermer apprendre choisir finir commencer continuer travailler chercher changer arriver partir revenir tenir compter tomber montrer gagner perdre acheter vendre payer recevoir rendre servir devenir connaître paraître permettre répondre offrir préparer écouter aider oublier rappeler rencontrer poser lever tourner utiliser garder construire conduire créer obtenir expliquer décider essayer inviter partager voyager rester histoire temps jour nuit matin soir semaine mois année heure minute aujourd'hui demain hier maintenant toujours jamais souvent parfois encore déjà bientôt ici là partout ailleurs avant après pendant depuis avec sans sous sur entre devant derrière dans contre vers chez pour par comme plus moins très bien mal beaucoup peu assez trop tout rien quelque chaque même autre petit grand bon beau nouveau vieux jeune long court haut bas vrai faux seul heureux triste facile difficile chaud froid fort doux clair sombre blanc noir rouge bleu vert jaune eau feu terre air ciel mer ville pays monde maison porte fenêtre table chaise lit livre école rue chemin jardin arbre fleur soleil lune étoile montagne rivière plage forêt pierre pain lait café thé pomme chat chien oiseau poisson cheval famille mère père frère sœur enfant fille garçon ami amour vie travail question réponse mot phrase histoire musique image couleur lumière voix place chose façon raison besoin idée fin début côté milieu fond ligne point nombre forme partie moment exemple hiver été printemps automne""",
    "german": """der die das ein eine eines einer einem einen und oder aber denn doch als wenn weil dass ob wie was wer wo wann warum ich du er sie es wir ihr mich dich sich mir dir uns euch mein dein sein unser euer ihr dieser diese dieses jener jene jedes jeder jede kein keine nicht noch schon nur auch immer nie oft manchmal heute morgen gestern jetzt hier dort überall vorher nachher sehr mehr weniger viel wenig alle alles etwas nichts man selbst zusammen wieder gern wirklich vielleicht haben sein werden können müssen wollen sollen dürfen machen sagen gehen kommen sehen wissen geben nehmen finden denken bleiben lassen stehen liegen sitzen halten bringen heißen sprechen fragen antworten lernen lesen schreiben arbeiten spielen leben lieben hören helfen suchen brauchen zeigen beginnen enden warten laufen fahren fliegen schwimmen essen trinken schlafen kaufen verkaufen bezahlen öffnen schließen treffen tragen ziehen legen setzen stellen bauen ändern erklären verstehen versuchen bekommen behalten vergessen verlieren gewinnen besuchen erzählen zählen fühlen wachsen fallen schauen wünschen glauben kennen Zeit Tag Nacht Morgen Abend Woche Monat Jahr Stunde Minute Welt Land Stadt Dorf Haus Tür Fenster Zimmer Tisch Stuhl Bett Buch Schule Straße Weg Garten Baum Blume Sonne Mond Stern Himmel Wasser Feuer Erde Luft Meer Berg Fluss Wald Stein Brot Milch Kaffee Tee Apfel Hund Katze Vogel Fisch Pferd Familie Mutter Vater Bruder Schwester Kind Tochter Sohn Freund Arbeit Frage Antwort Wort Satz Sprache Musik Bild Farbe Licht Stimme Platz Ding Sache Grund Idee Ende Anfang Seite Mitte Form Teil Moment Beispiel Winter Sommer Frühling Herbst Mensch Frau Mann Name Hand Kopf Auge Ohr Herz Fuß Bein Arm Haar Gesicht Leben Liebe Glück Geld Essen Wetter Reise Zug Auto Bus Boot Schiff Luft Tag gut schlecht schön neu alt jung groß klein lang kurz hoch tief wahr falsch leicht schwer warm kalt stark schwach hell dunkel weiß schwarz rot blau grün gelb schnell langsam ruhig laut früh spät weit nah gesund krank sauber offen leer voll rund gerade wichtig richtig fertig frei möglich bekannt gemeinsam besonders einfach genug anders beide nächste letzte erste zweite dritte""",
    "spanish": """el la los las un una unos unas de del a al y o pero porque que quien qué quién donde dónde cuando cuándo como cómo por para con sin sobre entre hasta desde durante contra hacia según yo tú él ella nosotros nosotras vosotros vosotras ellos ellas me te se nos os mi tu su mis tus sus nuestro nuestra vuestro vuestra este esta esto ese esa eso aquel aquella aquí allí allá ahora antes después hoy ayer mañana siempre nunca jamás también tampoco muy más menos mucho poco todo nada algo cada mismo otro alguno ninguno ser estar tener hacer decir ir ver dar saber querer poder deber venir poner salir llegar pasar quedar llevar dejar seguir encontrar llamar pensar sentir parecer volver tomar conocer vivir creer hablar contar esperar buscar entrar trabajar escribir leer comer beber dormir caminar correr abrir cerrar aprender elegir terminar empezar continuar cambiar ayudar olvidar recordar preguntar responder comprar vender pagar recibir servir ganar perder jugar escuchar mirar usar preparar viajar construir crear entender necesitar ofrecer compartir explicar decidir probar invitar reunir subir bajar caer crecer nacer morir tiempo día noche tarde semana mes año hora minuto mundo país ciudad pueblo casa puerta ventana habitación mesa silla cama libro escuela calle camino jardín árbol flor sol luna estrella cielo agua fuego tierra aire mar montaña río bosque piedra pan leche café té manzana perro gato pájaro pez caballo familia madre padre hermano hermana hijo hija niño niña amigo amiga hombre mujer persona nombre mano cabeza ojo oreja corazón pie pierna brazo pelo cara vida amor trabajo pregunta respuesta palabra frase idioma música imagen color luz voz lugar cosa razón idea fin principio lado medio forma parte momento ejemplo invierno verano primavera otoño bien mal bueno malo bonito nuevo viejo joven grande pequeño largo corto alto bajo verdadero falso fácil difícil caliente frío fuerte suave claro oscuro blanco negro rojo azul verde amarillo rápido lento tranquilo temprano tarde cerca lejos feliz triste sano enfermo limpio abierto cerrado vacío lleno redondo recto importante correcto listo libre posible conocido juntos especial sencillo bastante diferente primero segundo tercero último próximo mejor peor mayor menor""",
}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def canonical(words):
    return ("\n".join(words) + "\n").encode("utf-8")


def unique(values):
    return list(dict.fromkeys(values))


def source_form(word, language):
    if language == "french":
        mappings = {"é": "e'", "è": "e`", "ê": "e^", "ë": 'e"', "à": "a`", "â": "a^", "î": "i^", "ï": 'i"', "ô": "o^", "ù": "u`", "û": "u^", "ü": 'u"', "ç": "c,", "œ": "oe"}
    elif language == "german":
        mappings = {"ä": "ae", "ö": "oe", "ü": "ue", "ß": "ss", "Ä": "Ae", "Ö": "Oe", "Ü": "Ue"}
    else:
        mappings = {"á": "a`", "é": "e`", "í": "i`", "ó": "o`", "ú": "u`", "ñ": "\\", "ü": 'u"'}
    return "".join(mappings.get(c, c) for c in word)


def assemble(source_dir):
    manifest = json.loads((ROOT / "source-manifest.json").read_text())
    sources = {}
    for record in manifest["sources"]:
        data = (source_dir / record["filename"]).read_bytes()
        if sha(data) != record["sha256"]:
            raise ValueError("source checksum mismatch: " + record["filename"])
        # The historical German file contains a few unlabelled legacy-encoded
        # names. No selected token uses them; exclude those whole source lines.
        if record["id"] == "moby-german":
            sources[record["id"]] = "\n".join(line.decode("ascii") for line in data.splitlines() if line.isascii())
        else:
            sources[record["id"]] = data.decode("utf-8")

    freq = [word.strip().lower() for word in sources["moby-freq"].splitlines()[2:]]
    internet = [line.split()[-1].lower() for line in sources["moby-freq-int"].splitlines()[2:] if line.split()]
    ranked = unique(word for word in freq + internet if re.fullmatch("[a-z]+", word) and (len(word) > 1 or word in {"a", "i"}))
    assert len(ranked) >= 1000
    lists = {"english_200": ranked[:200], "english_1000": ranked[:1000]}
    common_ranked = set(ranked[:1000])
    dictionary = [word for word in sources["moby-common"].splitlines() if re.fullmatch("[a-z]{2,12}", word) and word not in common_ranked]
    # Select a spread of the common dictionary in a stable SHA-256 order; then
    # store alphabetically. This is a vocabulary sample, not a frequency claim.
    # Preserve the pre-rename seed so renaming the app never changes the vocabulary.
    dictionary = sorted(dictionary, key=lambda word: hashlib.sha256(("typ-english-10000-v1:" + word).encode()).digest())
    lists["english_10000"] = ranked[:1000] + sorted(dictionary[:9000])
    mappings = {}
    for language in CURATED:
        raw_words = set(sources["moby-" + language].splitlines())
        accepted = []
        mappings[language] = []
        for word in unique(CURATED[language].split()):
            raw = source_form(word, language)
            if raw in raw_words:
                accepted.append(unicodedata.normalize("NFC", word))
                mappings[language].append({"token": word, "source_form": raw})
        if len(accepted) < 200:
            raise ValueError(f"only {len(accepted)} verified {language} tokens")
        lists[language + "_200"] = accepted[:200]
        mappings[language] = mappings[language][:200]
    (ROOT / "language-source-mappings.json").write_text(json.dumps(mappings, ensure_ascii=False, indent=2) + "\n")

    language_tags = {"english": "en", "french": "fr", "german": "de", "spanish": "es"}
    for name, words in lists.items():
        assert len(words) == len(set(words)) == int(name.rsplit("_", 1)[1])
        directory = ROOT / "packs" / name
        directory.mkdir(parents=True, exist_ok=True)
        data = canonical(words)
        (directory / "words.txt").write_bytes(data)
        language = name.split("_")[0]
        book_id = 3201 if language == "english" else 3206
        metadata = {"schema_version": 1, "id": name, "language_tag": language_tags[language], "revision": "moby-clack-1", "source": f"Grady Ward, Moby {'Words' if language == 'english' else 'Language'} II; https://www.gutenberg.org/ebooks/{book_id}; selection and normalization by clack contributors", "license": "LicenseRef-Moby-Public-Domain", "content_hash": sha(data), "text_direction": "ltr", "supported_input_policy": "prose", "token_count": len(words)}
        (directory / "metadata.json").write_text(json.dumps(metadata, ensure_ascii=False, indent=2) + "\n")

    novels = {}
    for name, beginning in [("alice", "Alice was beginning"), ("pride", "It is a truth universally"), ("tale", "It was the best of times")]:
        text = sources[name]
        text = text[text.index(beginning):]
        novels[name] = [" ".join(paragraph.replace("_", "").split()) for paragraph in re.split(r"\n\s*\n", text)]
    selections = [
        ("pride-opening", "pride", [0]),
        ("alice-down", "alice", [3]),
        ("alice-book", "alice", [0]),
        ("pride-neighbourhood", "pride", [1]),
        ("tale-times", "tale", [0]),
        ("alice-rabbit", "alice", [0, 1]),
        ("tale-kingdoms", "tale", [0, 1]),
        ("alice-curiosity", "alice", [0, 1, 2]),
    ]
    authors = {"alice": ("Lewis Carroll", "Alice's Adventures in Wonderland", 11), "pride": ("Jane Austen", "Pride and Prejudice", 1342), "tale": ("Charles Dickens", "A Tale of Two Cities", 98)}
    quotes = []
    for quote_id, novel, paragraphs in selections:
        author, title, book_id = authors[novel]
        text = unicodedata.normalize("NFC", " ".join(novels[novel][index] for index in paragraphs))
        quotes.append({"id": quote_id, "author": author, "work": title, "source": f"https://www.gutenberg.org/ebooks/{book_id}", "license": "LicenseRef-Public-Domain-Literature", "revision": "clack-quotes-1", "content_hash": sha(text.encode()), "text": text})
    (ROOT / "quotes" / "quotes.json").write_text(json.dumps(quotes, ensure_ascii=False, indent=2) + "\n")
    for quote in quotes:
        print(quote["id"], len(quote["text"].split()))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", required=True, type=Path)
    assemble(parser.parse_args().source_dir)
