"""Generate tests/classical-knowledge-base/ — a large, link-dense
corpus on Greco-Roman antiquity (people, events, literature, places,
ideas) used for retrieval benchmarking.

Why a generator instead of hand-written notes:
- We want a couple of hundred files, all in the same frontmatter+wikilink
  format, with controlled link density so BM25 / dense / cortex all
  have something fair to chew on.
- The corpus should be reproducible — re-running this script overwrites
  the tree, so we can grow / refine entries without diff churn over
  formatting drift.

Each note has:
  - frontmatter tags (type/{person,event,work,place,concept,deity})
  - 4–8 sentences of *real content* (not a stub) so BM25 has signal
  - 4–10 wikilinks distributed naturally through the prose
  - an explicit "Connections" section listing every wikilink it makes,
    so a graph parser can lift the edges cheaply

This is hand-authored content baked into a script — not LLM-generated
filler. Edit the dicts below to refine, then re-run.
"""

from __future__ import annotations

import shutil
from pathlib import Path
from textwrap import dedent

ROOT = Path(__file__).resolve().parent.parent
KB = ROOT / "tests" / "classical-knowledge-base"


def slug(name: str) -> str:
    return name.lower().replace(" ", "-").replace("'", "").replace(".", "")


def note(
    title: str,
    kind: str,
    body: str,
    links: list[str],
    aliases: list[str] | None = None,
) -> str:
    """Render a single markdown note. `links` is the de-duplicated list
    of wikilink *targets* (slugs) referenced in body; we emit them in a
    Connections section so the graph extractor can ingest them without
    re-parsing prose."""
    front = ["---", f"tags: [type/{kind}]", "created: 2026-05-11"]
    if aliases:
        front.append(f"aliases: [{', '.join(aliases)}]")
    front.append("---")
    lines = ["\n".join(front), "", f"# {title}", "", dedent(body).strip(), ""]
    if links:
        lines += ["## Connections", ""]
        lines += [f"- [[{tgt}]]" for tgt in links]
        lines.append("")
    return "\n".join(lines)


# ─────────────────────────────────────────────────────────────────────────
# GREECE
# ─────────────────────────────────────────────────────────────────────────

GREECE_PEOPLE: dict[str, tuple[str, str, list[str]]] = {
    "socrates": (
        "person",
        """
        Socrates of Athens (c. 470–399 BCE) was a classical Greek philosopher
        credited as a founder of Western moral philosophy. The son of a
        stonemason and a midwife, he served as a hoplite at Potidaea (432),
        Delium (424), and Amphipolis (422) during the [[peloponnesian-war]]
        — [[plato]] credits him with saving Alcibiades' life at Potidaea.
        He left no writings; what we know of him comes through his students
        [[plato]] and [[xenophon]], and through the comedies of
        [[aristophanes]], whose Clouds (423 BCE) lampooned him as a
        sophistic head-in-the-clouds natural philosopher — a caricature
        Plato later argued contributed to public prejudice against him.
        His method — the elenchus — proceeded by relentless cross-
        examination of interlocutors in the [[athens|Athenian]] agora,
        exposing contradictions in claims about virtue (arete), justice,
        and piety. After the war and the brief tyranny of the Thirty (some
        of whom, including Critias and Charmides, had been his associates),
        a restored [[democracy]] tried him in 399 BCE on charges of impiety
        and corrupting the youth. Convicted by a narrow vote of a 500-man
        jury and refusing the customary escape into exile, he drank
        hemlock. His trial is narrated in [[plato|Plato's]] [[apology]] and
        his death in the [[phaedo]]; [[xenophon]] also wrote his own
        Apology and Memorabilia.
        """,
        ["peloponnesian-war", "plato", "xenophon", "aristophanes", "athens", "democracy", "apology", "phaedo"],
    ),
    "plato": (
        "person",
        """
        Plato (c. 428–348 BCE), born Aristocles into an old [[athens|Athenian]]
        aristocratic family connected to the oligarchic Thirty, became the
        pupil of [[socrates]] and, after Socrates' execution in 399 BCE,
        traveled to southern Italy and Sicily where he encountered
        Pythagorean mathematics — a contact that visibly shapes the
        [[timaeus]]. He founded the Academy in the grove of Akademos
        outside Athens around 387 BCE; it would operate, in various forms,
        for nearly nine centuries until [[justinian]] closed the pagan
        philosophical schools in 529 CE. His thirty-five extant dialogues
        and thirteen letters cover almost every topic in philosophy:
        political theory ([[republic]], Statesman, Laws), metaphysics
        ([[timaeus]], Parmenides), epistemology (Theaetetus, Meno),
        philosophy of language (Cratylus), eros and beauty ([[symposium]],
        Phaedrus), and the conduct of the philosophical life ([[apology]],
        [[phaedo]], Crito). His theory of the [[forms|Forms]] holds that
        the changing particulars of sense are imperfect instances of
        eternal universals; his tripartite soul (reason / spirit /
        appetite) underlies the Republic's analogous tripartite city.
        Three trips to Syracuse (387, 367, 361) trying to educate the
        younger Dionysius into a philosopher-king ended badly each time.
        His pupil [[aristotle]] would break with him on the Forms while
        carrying forward the project of systematic philosophy.
        """,
        ["athens", "socrates", "timaeus", "justinian", "republic", "symposium", "apology", "phaedo", "forms", "aristotle"],
    ),
    "aristotle": (
        "person",
        """
        Aristotle (384–322 BCE), born in Stagira on the Chalcidic peninsula
        to Nicomachus — court physician to the Macedonian king Amyntas III
        — entered [[plato|Plato's]] Academy at seventeen and remained
        twenty years until Plato's death. Passed over for the headship of
        the Academy, he traveled to Assos and Lesbos doing some of the
        first systematic field biology in history (the marine zoology of
        the Lagoon of Pyrrha). Around 343 BCE [[philip-ii]] summoned him
        to Pella to tutor the thirteen-year-old [[alexander-the-great]]. He
        founded the Lyceum (the Peripatetic school) in [[athens]] in 335
        BCE on Alexander's accession, and fled the city in 322 after
        Alexander's death to escape an anti-Macedonian backlash —
        reportedly saying he would not let Athens "sin twice against
        philosophy," in reference to [[socrates]]. His surviving works
        (likely lecture notes assembled by later editors, principally
        Andronicus of Rhodes c. 60 BCE) cover logic (the Organon),
        physics, biology, the soul (De Anima), metaphysics, ethics
        ([[nicomachean-ethics]], Eudemian Ethics), [[politics-aristotle|Politics]],
        rhetoric, and the [[poetics]]. His empirical bent broke with
        Plato's [[forms|Forms]]: universals are instantiated *in* things,
        not in a separate realm. Recovered through Syriac and Arabic
        intermediaries (notably Averroes), his works became the spine of
        medieval scholasticism — "the Philosopher" tout court in Aquinas.
        """,
        ["plato", "philip-ii", "alexander-the-great", "athens", "socrates", "nicomachean-ethics", "politics-aristotle", "poetics", "forms"],
    ),
    "alexander-the-great": (
        "person",
        """
        Alexander III of [[macedon]] (356–323 BCE), son of [[philip-ii]]
        and Olympias of Epirus, was tutored from age thirteen by
        [[aristotle]] at Mieza. He acceded at twenty after Philip's
        assassination in 336 BCE, crushed a Theban revolt by levelling the
        city in 335 (sparing only the house of Pindar), and crossed the
        Hellespont in 334 BCE with roughly 40,000 troops to launch the
        invasion of Persia his father had planned. Four major battles
        decided the empire: the Granicus (334), [[battle-of-issus|Issus]]
        (333) against [[darius-iii]] in person, [[battle-of-gaugamela|Gaugamela]]
        (331) — the decisive engagement — and the Hydaspes (326) against
        the Indian king Porus. Between these he took Tyre after a seven-
        month siege, founded [[alexandria]] in Egypt (331 BCE) after being
        hailed son of Ammon at Siwa, burned Persepolis in 330 (whether
        drunken revenge for Xerxes or a calculated political signal is
        ancient), and pursued Darius into Bactria where the satrap Bessus
        murdered him. Adopting Persian dress and court ritual (proskynesis)
        and marrying Roxana, then Stateira and Parysatis at Susa, he
        alienated his Macedonian veterans; their mutiny at the Hyphasis in
        326 turned the army back from India. He died of fever (or
        poisoning, by some accounts) in Babylon in June 323 BCE at age 32.
        He founded perhaps two dozen cities named Alexandria from Egypt to
        the Hindu Kush; his empire fractured into the [[hellenistic-age|Hellenistic]]
        successor kingdoms of the Diadochi — [[ptolemy-i|Ptolemy]] taking
        Egypt, Seleucus taking the eastern bulk, Antigonus and his line
        Macedon — and defined the political map of the eastern
        Mediterranean for three centuries. Principal sources: Arrian's
        Anabasis (drawing on the lost memoirs of [[ptolemy-i]] and
        Aristobulus), [[plutarch|Plutarch's]] Life, Quintus Curtius Rufus,
        Diodorus Siculus.
        """,
        ["macedon", "philip-ii", "aristotle", "battle-of-issus", "battle-of-gaugamela", "darius-iii", "alexandria", "hellenistic-age", "ptolemy-i", "plutarch"],
    ),
    "philip-ii": (
        "person",
        """
        Philip II of [[macedon]] (382–336 BCE), father of [[alexander-the-great]],
        transformed a marginal kingdom into the dominant power of mainland
        Greece. His reformed phalanx with sarissa pikes broke the Theban
        Sacred Band at Chaeronea (338 BCE), ending the era of the
        independent [[polis]]. He was assassinated at his daughter's wedding
        before launching a planned Persian invasion — which his son
        completed instead.
        """,
        ["macedon", "alexander-the-great", "polis"],
    ),
    "pericles": (
        "person",
        """
        Pericles (c. 495–429 BCE) was the dominant statesman of [[athens]]
        during its golden age. He sponsored the rebuilding of the Acropolis
        and the Parthenon, expanded [[democracy]] through pay for jury
        service, and led Athens into the [[peloponnesian-war]] against
        [[sparta]]. He died in the plague that swept Athens in the war's
        second year, leaving the city without his guiding hand for the
        catastrophe that followed. His funeral oration, preserved by
        [[thucydides]], is the canonical statement of democratic ideology.
        """,
        ["athens", "democracy", "peloponnesian-war", "sparta", "thucydides"],
    ),
    "leonidas": (
        "person",
        """
        Leonidas I of [[sparta]] (died 480 BCE) led the Greek rearguard at
        the [[battle-of-thermopylae]] during the second Persian invasion.
        His three hundred Spartiates, alongside Thespian and Theban allies,
        held the pass long enough for the Greek fleet to regroup and for
        [[themistocles]] to engineer the decisive naval victory at
        [[battle-of-salamis|Salamis]]. The stand became the founding
        narrative of Western resistance to overwhelming force.
        """,
        ["sparta", "battle-of-thermopylae", "themistocles", "battle-of-salamis"],
    ),
    "themistocles": (
        "person",
        """
        Themistocles (c. 524–459 BCE) was the [[athens|Athenian]] statesman
        who built the silver-funded fleet that won the [[battle-of-salamis]]
        and effectively ended the Persian threat to Greece. He understood
        before anyone else that Athens' future was naval. He was later
        ostracized and ended his life as a Persian satrap of Magnesia — the
        savior of Greece on a Persian pension.
        """,
        ["athens", "battle-of-salamis"],
    ),
    "solon": (
        "person",
        """
        Solon (c. 630–560 BCE) was an [[athens|Athenian]] lawgiver whose
        reforms — debt cancellation, abolition of debt-slavery, and a
        property-based class system replacing pure birth — laid the
        groundwork for [[democracy]]. He is counted among the Seven Sages of
        Greece and his constitutional framework, though aristocratic by
        modern standards, was radically egalitarian for the 6th century BCE.
        """,
        ["athens", "democracy"],
    ),
    "draco": (
        "person",
        """
        Draco (7th century BCE) was the first recorded Athenian lawgiver,
        whose written code prescribed death for most offenses — giving us
        the adjective "draconian." His laws preceded [[solon|Solon's]]
        reforms and were almost entirely repealed by them, surviving only in
        the homicide statutes.
        """,
        ["solon"],
    ),
    "herodotus": (
        "person",
        """
        Herodotus of Halicarnassus (c. 484–425 BCE), called "the Father of
        History" by [[cicero]], wrote the [[histories]] — a vast inquiry
        into the [[persian-wars]] interwoven with ethnographies of Egypt,
        Scythia, Lydia, and Persia. His method mixed eyewitness testimony,
        oral tradition, and frank credulity, but the impulse to *ask why* a
        war happened — and to seek causes in customs, geography, and human
        character — was new in the world.
        """,
        ["cicero", "histories", "persian-wars"],
    ),
    "thucydides": (
        "person",
        """
        Thucydides (c. 460–400 BCE) wrote the *History of the
        [[peloponnesian-war|Peloponnesian War]]*, a self-consciously
        political and rigorous successor to [[herodotus]]. An Athenian
        general exiled after losing Amphipolis, he had time to interview
        survivors on both sides. The Melian Dialogue and the Mytilenian
        Debate are still taught in international-relations curricula as
        foundational texts on power and justice.
        """,
        ["peloponnesian-war", "herodotus"],
    ),
    "xenophon": (
        "person",
        """
        Xenophon (c. 430–354 BCE) was an [[athens|Athenian]] soldier and
        student of [[socrates]] who marched with the Ten Thousand Greek
        mercenaries deep into Persia under Cyrus the Younger. His
        *Anabasis* — the "march up country" — narrates their fighting
        retreat to the Black Sea and is the first European war memoir. He
        also wrote on Socrates, on horsemanship, and on the education of
        [[cyrus-the-great|Cyrus]].
        """,
        ["athens", "socrates", "cyrus-the-great"],
    ),
    "pythagoras": (
        "person",
        """
        Pythagoras of Samos (c. 570–495 BCE) founded a religious-philosophical
        brotherhood in southern Italy that combined mathematics, music
        theory, vegetarianism, and metempsychosis. Whether he proved "his"
        theorem is uncertain; the school's discovery that musical intervals
        correspond to simple integer ratios was the first inkling that the
        cosmos might be intelligible through number — an idea that runs
        through [[plato]] and into modern physics.
        """,
        ["plato"],
    ),
    "archimedes": (
        "person",
        """
        Archimedes of Syracuse (c. 287–212 BCE) was the greatest mathematician
        and engineer of antiquity. He approximated π, derived the volume of
        the sphere, and designed war machines that held off the Roman siege
        of Syracuse during the [[second-punic-war]] until a soldier killed
        him while he was drawing diagrams in the sand.
        """,
        ["second-punic-war"],
    ),
    "euclid": (
        "person",
        """
        Euclid of [[alexandria]] (fl. 300 BCE) compiled the *Elements*, a
        thirteen-book systematization of geometry and number theory that
        served as the standard mathematics textbook for over two millennia.
        Its axiomatic method — definitions, postulates, common notions,
        propositions — became the template for rigorous demonstration in
        every later discipline.
        """,
        ["alexandria"],
    ),
    "hippocrates": (
        "person",
        """
        Hippocrates of Kos (c. 460–370 BCE) is the namesake of the
        Hippocratic Corpus and the oath. The corpus broke with priestly
        medicine by insisting on natural causes for illness — the four
        humors framework, however wrong, was an empirical move. The
        "Hippocratic" of the [[philosophy]] tradition is the same impulse
        that drove [[aristotle|Aristotle's]] biology.
        """,
        ["philosophy", "aristotle"],
    ),
    "diogenes": (
        "person",
        """
        Diogenes of Sinope (c. 412–323 BCE) was the founding figure of
        [[cynicism]]. He lived in a wine jar in [[athens]], owned nothing,
        and is said to have asked [[alexander-the-great]] to stand out of
        his sunlight when the conqueror came to honor him. The Cynic
        rejection of convention as a path to virtue prefigured [[stoicism]].
        """,
        ["cynicism", "athens", "alexander-the-great", "stoicism"],
    ),
    "epicurus": (
        "person",
        """
        Epicurus (341–270 BCE) founded the Garden in [[athens]] and the
        philosophy of [[epicureanism]]: pleasure as the absence of pain
        (ataraxia), atoms moving in the void, and gods unconcerned with
        human affairs. The Roman poet [[lucretius]] later versified the
        Epicurean physics in *[[de-rerum-natura]]*.
        """,
        ["athens", "epicureanism", "lucretius", "de-rerum-natura"],
    ),
    "zeno-of-citium": (
        "person",
        """
        Zeno of Citium (c. 334–262 BCE) founded [[stoicism]] in the Painted
        Stoa of [[athens]] after a shipwreck stranded him there. The
        Stoic doctrine — virtue is the only good, the cosmos is rational,
        emotions are judgments to be corrected — would dominate elite
        Roman ethics through [[seneca]], [[epictetus]], and
        [[marcus-aurelius]].
        """,
        ["stoicism", "athens", "seneca", "epictetus", "marcus-aurelius"],
    ),
    "epictetus": (
        "person",
        """
        Epictetus (c. 50–135 CE) was a Greek Stoic philosopher born a slave
        and freed in [[rome]]. His *Discourses*, transcribed by his student
        Arrian, distilled [[stoicism]] into a discipline of distinguishing
        what is and is not in one's power. He influenced
        [[marcus-aurelius]] directly.
        """,
        ["rome", "stoicism", "marcus-aurelius"],
    ),
    "cleisthenes": (
        "person",
        """
        Cleisthenes of [[athens]] (c. 570–508 BCE), of the Alcmaeonid clan,
        is conventionally called the "father of [[democracy|Athenian
        democracy]]." After the expulsion of the Peisistratid tyrants
        (510 BCE) and a brief Spartan-backed counter-coup, he overhauled
        the constitution in 508/507 BCE: the four traditional Ionian
        tribes were replaced by ten new ones whose membership was
        deliberately scrambled across coast, city, and inland trittyes,
        breaking up regional aristocratic power. He created the Council
        of 500 (the boule, fifty men from each tribe by lot) and is
        traditionally credited with introducing ostracism — the annual
        ten-year exile of any citizen by majority vote. The reforms
        followed and built on [[solon|Solon's]]; [[pericles]] later
        extended them.
        """,
        ["athens", "democracy", "solon", "pericles"],
    ),
    "demosthenes": (
        "person",
        """
        Demosthenes (384–322 BCE) was the greatest Athenian orator of the
        4th century BCE, famous for the Philippics — speeches urging
        [[athens]] to resist [[philip-ii|Philip of Macedon]] before he
        absorbed Greece. He overcame a childhood speech impediment by
        speaking with pebbles in his mouth (so [[plutarch]] reports) and
        practiced declamation against the sea. Athens lost at Chaeronea
        (338 BCE) and the Macedonian conquest he had warned against
        proceeded. After [[alexander-the-great|Alexander's]] death he
        led one last rising; defeated, he took poison in the temple of
        Poseidon on Calauria in 322 BCE rather than be captured.
        """,
        ["athens", "philip-ii", "plutarch", "alexander-the-great"],
    ),
    "plutarch": (
        "person",
        """
        Plutarch of Chaeronea (c. 46–120 CE) was a Greek-speaking Roman
        citizen, priest at Delphi, and the most influential biographer of
        antiquity. His *Parallel Lives* paired forty-eight Greek and
        Roman statesmen (Alexander/Caesar, Demosthenes/Cicero, Theseus/
        Romulus, etc.) to draw out moral character. His *Moralia* is a
        large miscellany of ethical, religious, and literary essays.
        Shakespeare's [[julius-caesar|Caesar]], Antony and Cleopatra, and
        Coriolanus draw directly from North's English translation of
        Plutarch.
        """,
        ["julius-caesar"],
    ),
    "polybius": (
        "person",
        """
        Polybius of Megalopolis (c. 200–118 BCE), Greek historian, was
        deported to [[rome]] after the Roman victory at Pydna (168 BCE)
        as one of a thousand Achaean hostages. He befriended
        [[scipio-aemilianus]] and witnessed the destruction of [[carthage]]
        and Corinth in 146 BCE. His *Histories* (forty books, five
        surviving entire) explain Rome's rise to Mediterranean dominance
        between the [[first-punic-war]] and the destruction of Carthage,
        introducing the doctrine of the "mixed constitution" — Rome's
        balance of monarchical (consuls), aristocratic (Senate), and
        democratic (assemblies) elements — that would influence
        Montesquieu and the framers of the U.S. Constitution.
        """,
        ["rome", "scipio-aemilianus", "carthage", "first-punic-war"],
    ),
    "marius": (
        "person",
        """
        Gaius Marius (157–86 BCE), Roman general and consul a record
        seven times. His military reforms — opening recruitment to the
        landless capite censi and standardizing the cohort — created a
        professional army loyal to its commander rather than the state,
        with consequences neither he nor anyone else foresaw. He defeated
        Jugurtha in North Africa and the Cimbri and Teutones in the
        north (102–101 BCE). His bitter rivalry with his former
        lieutenant [[sulla]] launched the first Roman civil war and the
        cycle of proscriptions that ended with [[augustus]].
        """,
        ["sulla", "augustus"],
    ),
    "sulla": (
        "person",
        """
        Lucius Cornelius Sulla Felix (138–78 BCE), Roman general and
        dictator. Quaestor under [[marius]] in the Jugurthine War, he
        broke with his patron and led the first Roman army ever to march
        on [[rome]] (88 BCE), then again in 82 BCE after his eastern
        campaign against Mithridates VI of Pontus. Victorious in civil
        war, he was named dictator legibus faciendis et rei publicae
        constituendae causa — a position with no time limit — and used
        it to issue proscriptions that killed thousands of his enemies,
        then to enact constitutional reforms strengthening the Senate.
        He stunned contemporaries by voluntarily retiring in 79 BCE.
        His career was the template a young [[julius-caesar]] both
        feared and studied.
        """,
        ["marius", "rome", "julius-caesar"],
    ),
    "gracchi-brothers": (
        "person",
        """
        Tiberius Gracchus (c. 163–133 BCE) and Gaius Gracchus (c. 154–121
        BCE), tribunes of the plebs whose attempts at land reform —
        redistributing public land (ager publicus) to landless citizens
        — opened the long political crisis of the late Roman Republic.
        Tiberius was clubbed to death on the Capitoline in 133 BCE by a
        senatorial mob led by his cousin Scipio Nasica; Gaius, pursuing
        broader reforms a decade later, was driven to suicide in 121 BCE
        after the Senate's first *senatus consultum ultimum*. They are
        the canonical first casualties of the Republic's collapse into
        [[fall-of-the-republic|civil violence]].
        """,
        ["fall-of-the-republic"],
    ),
    "cato-the-elder": (
        "person",
        """
        Marcus Porcius Cato (234–149 BCE), known as Cato the Censor, was
        a Roman statesman famous for moralizing severity, anti-Hellenic
        conservatism, and ending every Senate speech with *Carthago
        delenda est* — Carthage must be destroyed — until [[carthage]]
        was destroyed in the [[third-punic-war]] the year he died.
        """,
        ["carthage", "third-punic-war"],
    ),
    "cato-the-younger": (
        "person",
        """
        Marcus Porcius Cato Uticensis (95–46 BCE), great-grandson of
        [[cato-the-elder]], was the most prominent senatorial opponent
        of [[julius-caesar]]. A devout [[stoicism|Stoic]] of legendary
        incorruptibility, he committed suicide at Utica in 46 BCE after
        Caesar's victory at Thapsus rather than accept a pardon. He
        became the Stoic martyr-figure of the lost Republic for
        [[seneca]] and Lucan.
        """,
        ["cato-the-elder", "julius-caesar", "stoicism", "seneca"],
    ),
    "pyrrhus": (
        "person",
        """
        Pyrrhus of Epirus (c. 319–272 BCE), Hellenistic king and cousin
        of [[alexander-the-great]], invaded Italy in 280 BCE at the
        invitation of Tarentum to defend the Greek cities against
        [[rome]]. He defeated the Romans at Heraclea (280) and Asculum
        (279) but at such cost that he reportedly said "another such
        victory and I am undone" — the origin of "Pyrrhic victory."
        Withdrew to Sicily, then back to Epirus, and was killed by a
        roof tile in a street fight at Argos. His Italian war was
        Rome's first against a major Hellenistic power and a preview of
        the wars to come.
        """,
        ["alexander-the-great", "rome"],
    ),
    "phidias": (
        "person",
        """
        Phidias (c. 480–430 BCE) was the supreme sculptor of classical
        [[athens]], overseer of the Parthenon's sculptural program under
        [[pericles]] and creator of the colossal chryselephantine statues
        of Athena Parthenos (in the Parthenon) and Zeus at Olympia (one
        of the Seven Wonders). All his major works are lost; their
        impact on Roman sculptural taste is preserved through copies.
        """,
        ["athens", "pericles"],
    ),
    "praxiteles": (
        "person",
        """
        Praxiteles (fl. 4th century BCE), Athenian sculptor of the later
        classical period, whose Aphrodite of Knidos was the first
        monumental female nude in Greek art and the most copied statue
        of antiquity.
        """,
        [],
    ),
    "lysippus": (
        "person",
        """
        Lysippus of Sicyon (4th century BCE), favored sculptor of
        [[alexander-the-great]], introduced a leaner, more elongated
        canon than Polykleitos' fifth-century proportions. The Farnese
        Hercules and the Apoxyomenos are Roman copies of his work.
        """,
        ["alexander-the-great"],
    ),
}

GREECE_WRITERS: dict[str, tuple[str, str, list[str]]] = {
    "homer": (
        "person",
        """
        Homer is the traditional author of the [[iliad]] and the [[odyssey]],
        the two foundational epics of Greek literature. Whether he was a
        single 8th-century-BCE poet, a tradition, or several bards working
        an oral inheritance is the "Homeric Question." Either way, these
        poems are the bedrock — [[plato]] called Homer the educator of all
        Greece, then proposed banning him from the [[republic|ideal city]].
        """,
        ["iliad", "odyssey", "plato", "republic"],
    ),
    "hesiod": (
        "person",
        """
        Hesiod (fl. c. 700 BCE) was a Boeotian poet whose [[theogony]]
        systematized the genealogy of the Greek gods and whose *Works and
        Days* gave a peasant-farmer ethics of toil, justice, and seasons.
        Where [[homer]] sings of heroes, Hesiod sings of plowmen and the
        moral economy of a poor village.
        """,
        ["theogony", "homer"],
    ),
    "sappho": (
        "person",
        """
        Sappho of Lesbos (c. 630–570 BCE) was the foremost lyric poet of
        archaic Greece, whose surviving fragments are among the most direct
        and personal voices from antiquity. [[plato|Plato]] reputedly called
        her "the tenth Muse." Most of her corpus is lost, preserved only
        through citations in later grammarians.
        """,
        ["plato"],
    ),
    "pindar": (
        "person",
        """
        Pindar (c. 518–438 BCE) was the great choral lyric poet of Greece,
        famed for his victory odes celebrating winners at the Olympic and
        Pythian games. His dense, allusive style was the gold standard for
        high lyric in antiquity.
        """,
        [],
    ),
    "aeschylus": (
        "person",
        """
        Aeschylus (c. 525–456 BCE) is the earliest of the three great
        Athenian tragedians, fighter at the [[battle-of-marathon]] and
        author of the [[oresteia]] — the only surviving trilogy from Greek
        tragedy. He added a second actor to the stage, opening drama up to
        real dialogue.
        """,
        ["battle-of-marathon", "oresteia"],
    ),
    "sophocles": (
        "person",
        """
        Sophocles (c. 497–406 BCE) wrote [[oedipus-rex]], [[antigone]], and
        five other surviving tragedies. He added a third actor, deepened
        characterization, and is the tragedian on whom [[aristotle|Aristotle]]
        modeled his theory of tragedy in the Poetics. His Antigone — duty to
        the gods against duty to the state — is the perennial school text
        on civil disobedience.
        """,
        ["oedipus-rex", "antigone", "aristotle"],
    ),
    "euripides": (
        "person",
        """
        Euripides (c. 480–406 BCE), youngest of the great Athenian tragedians,
        wrote *Medea*, *The Bacchae*, *The Trojan Women* and seventeen other
        surviving plays — more than [[aeschylus]] and [[sophocles]] combined.
        He brought psychological realism, gave voice to women and slaves,
        and questioned the gods more openly than his peers.
        """,
        ["aeschylus", "sophocles"],
    ),
    "aristophanes": (
        "person",
        """
        Aristophanes (c. 446–386 BCE) was the great comic playwright of
        [[athens]], whose surviving comedies — *The Clouds*, *The Birds*,
        *Lysistrata*, *The Frogs* — savage [[socrates]], [[euripides]], war,
        and the [[democracy|Athenian assembly]] alike. Old Comedy at its
        sharpest.
        """,
        ["athens", "socrates", "euripides", "democracy"],
    ),
}

GREECE_WORKS: dict[str, tuple[str, str, list[str]]] = {
    "iliad": (
        "work",
        """
        The Iliad is [[homer|Homer's]] epic of the wrath of Achilles in the
        ninth year of the [[trojan-war]]. Its action covers only weeks of
        the ten-year siege but encompasses the death of Patroclus, the
        slaying of Hector, and Priam's night journey to ransom his son's
        body. The poem's themes — honor, mortality, the cost of glory —
        established the vocabulary of Western heroic literature. [[achilles]]
        and [[hector]] are its twin centers.
        """,
        ["homer", "trojan-war", "achilles", "hector"],
    ),
    "odyssey": (
        "work",
        """
        The Odyssey, [[homer|Homer's]] second epic, follows [[odysseus]] on
        his ten-year voyage home from the [[trojan-war]] to Ithaca and his
        long-suffering wife Penelope. Episodes with the Cyclops, Circe, the
        Sirens, and the descent to the underworld defined the structure of
        the literary journey for everyone after — including [[virgil]] in
        the [[aeneid]] and James Joyce three millennia later.
        """,
        ["homer", "odysseus", "trojan-war", "virgil", "aeneid"],
    ),
    "theogony": (
        "work",
        """
        [[hesiod|Hesiod's]] Theogony narrates the origin of the cosmos and
        the genealogy of the gods from Chaos through Gaia, the Titans, and
        the Olympians under [[zeus]]. It is the canonical Greek creation
        myth, the source most later authors silently rely on.
        """,
        ["hesiod", "zeus"],
    ),
    "oedipus-rex": (
        "work",
        """
        [[sophocles|Sophocles']] Oedipus Tyrannus is the archetypal Greek
        tragedy: a king discovers, through his own relentless inquiry, that
        he has killed his father and married his mother. [[aristotle]]
        treats it in the Poetics as the model of tragic plot construction.
        """,
        ["sophocles", "aristotle"],
    ),
    "antigone": (
        "work",
        """
        [[sophocles|Sophocles']] Antigone stages a collision between divine
        law (the duty to bury one's brother) and civic law (the king's edict
        forbidding it). Hegel made it the paradigm of tragic conflict —
        right against right, not right against wrong.
        """,
        ["sophocles"],
    ),
    "oresteia": (
        "work",
        """
        The Oresteia, [[aeschylus|Aeschylus']] surviving trilogy, follows
        the house of Atreus through Agamemnon's return, his murder by
        Clytemnestra, Orestes' revenge, and his trial by the new Athenian
        court of the Areopagus. It dramatizes the replacement of blood
        vengeance by civic justice — a foundational text for Athenian
        political identity.
        """,
        ["aeschylus"],
    ),
    "republic": (
        "work",
        """
        [[plato|Plato's]] Republic constructs the ideal city as a magnifying
        glass for the just soul, introduces the tripartite psyche, the
        philosopher-king, the allegories of the Sun, Line, and [[cave|Cave]],
        and a critique of [[democracy]] that has been argued with ever
        since. It is the founding text of Western political philosophy.
        """,
        ["plato", "cave", "democracy"],
    ),
    "symposium": (
        "work",
        """
        [[plato|Plato's]] Symposium is a dialogue on the nature of love
        (eros), set at a drinking party with [[socrates]], [[aristophanes]],
        the tragedian Agathon, and others each delivering a speech. Diotima's
        ladder of love — from particular bodies up to the [[forms|Form]] of
        Beauty itself — is the dialogue's metaphysical climax.
        """,
        ["plato", "socrates", "aristophanes", "forms"],
    ),
    "phaedo": (
        "work",
        """
        [[plato|Plato's]] Phaedo recounts the last day of [[socrates]],
        including his arguments for the immortality of the soul and the
        scene of his death by hemlock. It is the canonical philosophical
        martyrdom.
        """,
        ["plato", "socrates"],
    ),
    "apology": (
        "work",
        """
        [[plato|Plato's]] Apology is [[socrates|Socrates']] defense speech
        at his trial — not an apology in the modern sense but a
        justification. "The unexamined life is not worth living" is its
        most quoted line.
        """,
        ["plato", "socrates"],
    ),
    "timaeus": (
        "work",
        """
        [[plato|Plato's]] Timaeus is his cosmological dialogue, in which a
        Pythagorean explains the construction of the universe by a divine
        Demiurge using mathematical proportions and the four elements. The
        dialogue's account of Atlantis launched a small industry of pseudo-
        history.
        """,
        ["plato"],
    ),
    "nicomachean-ethics": (
        "work",
        """
        [[aristotle|Aristotle's]] Nicomachean Ethics develops the doctrine
        of virtue as a habituated mean between extremes, oriented toward
        eudaimonia — flourishing — as the highest human good. The treatise
        founded virtue ethics, which has had a substantial revival in
        contemporary moral philosophy.
        """,
        ["aristotle"],
    ),
    "histories": (
        "work",
        """
        [[herodotus|Herodotus']] Histories, in nine books named for the
        Muses, narrates the [[persian-wars]] alongside vast ethnographic
        digressions on Egypt, Scythia, Lydia, and Persia. It is the
        original work of inquiry — *historia* — into the human past.
        """,
        ["herodotus", "persian-wars"],
    ),
    "poetics": (
        "work",
        """
        [[aristotle|Aristotle's]] Poetics is the founding work of Western
        literary theory. Surviving in a single book (a second on comedy
        is lost), it analyzes tragedy as the imitation (mimesis) of a
        serious action that arouses pity and fear and effects catharsis
        of those emotions. Aristotle treats [[oedipus-rex]] as the
        exemplary plot, codifies the unities (only unity of action is
        explicitly Aristotelian — the others are Renaissance accretions),
        and distinguishes the universal in poetry from the particular in
        history.
        """,
        ["aristotle", "oedipus-rex"],
    ),
    "politics-aristotle": (
        "work",
        """
        [[aristotle|Aristotle's]] Politics, eight books, founds political
        science as systematic inquiry. Famously defines the human being as
        a *zoon politikon* — a political animal — surveys constitutions,
        classifies regimes by who rules and in whose interest, and
        defends polity (a mixed constitution of oligarchic and democratic
        elements) as most stable. Includes the notorious defense of
        natural slavery in book I, which would be cited by both
        proponents and opponents of slavery in the early-modern Atlantic.
        """,
        ["aristotle"],
    ),
}

GREECE_EVENTS: dict[str, tuple[str, str, list[str]]] = {
    "trojan-war": (
        "event",
        """
        The Trojan War, the great event of Greek myth-history, was the
        ten-year siege of Troy by a coalition of Greek kings to recover
        Helen. Its narrative is preserved in the [[iliad]] and the
        [[odyssey]] and many lost epics of the Cycle. [[achilles]],
        [[hector]], [[odysseus]], and Agamemnon are its central figures.
        Whether a historical Bronze Age conflict underlies the myth
        remains debated; the city Schliemann excavated at Hisarlik was
        certainly destroyed multiple times.
        """,
        ["iliad", "odyssey", "achilles", "hector", "odysseus"],
    ),
    "persian-wars": (
        "event",
        """
        The Persian Wars (499–449 BCE) pitted a coalition of Greek poleis,
        led intermittently by [[athens]] and [[sparta]], against the
        Achaemenid Empire of Darius and Xerxes. Decisive moments include
        the [[battle-of-marathon]] (490), [[battle-of-thermopylae]] (480),
        [[battle-of-salamis]] (480), and Plataea (479). [[herodotus]] is
        the principal source.
        """,
        ["athens", "sparta", "battle-of-marathon", "battle-of-thermopylae", "battle-of-salamis", "herodotus"],
    ),
    "battle-of-marathon": (
        "event",
        """
        At Marathon (490 BCE), an [[athens|Athenian]] force under Miltiades
        defeated Darius' invading Persians on the plain northeast of the
        city. The legendary run of Pheidippides to announce the victory
        gave us the modern footrace. [[aeschylus]] fought in the battle and
        had it inscribed on his epitaph.
        """,
        ["athens", "aeschylus"],
    ),
    "battle-of-thermopylae": (
        "event",
        """
        At Thermopylae (480 BCE), [[leonidas|Leonidas']] three hundred
        Spartiates with allied contingents held the narrow pass against
        Xerxes' invading army for three days before being outflanked and
        annihilated. The stand bought time for the Greek fleet and
        immortalized [[sparta|Sparta's]] military ethos.
        """,
        ["leonidas", "sparta"],
    ),
    "battle-of-salamis": (
        "event",
        """
        At Salamis (480 BCE), the Greek fleet under [[themistocles]] lured
        Xerxes' larger Persian navy into the narrow strait and destroyed
        it. Xerxes watched from a hillside. The victory effectively ended
        the Persian invasion of Greece — though land fighting continued
        another year at Plataea.
        """,
        ["themistocles"],
    ),
    "peloponnesian-war": (
        "event",
        """
        The Peloponnesian War (431–404 BCE) was the long, ruinous conflict
        between the [[athens|Athenian]] maritime empire under [[pericles]]
        and the [[sparta|Spartan]] land coalition. [[thucydides]] wrote
        its history. Athens fell, the empire dissolved, and the political
        confidence that had produced the golden age of tragedy and
        philosophy gave way to the bitter [[athens|Athens]] that condemned
        [[socrates]] in 399.
        """,
        ["athens", "pericles", "sparta", "thucydides", "socrates"],
    ),
    "battle-of-issus": (
        "event",
        """
        At Issus (November 333 BCE), [[alexander-the-great]] defeated
        [[darius-iii]] in person on a narrow coastal plain in Cilicia.
        Darius' superior numbers could not be brought to bear; his
        flight from the battlefield became iconic (the Alexander Mosaic
        from Pompeii captures the moment). Alexander captured Darius'
        wife, mother, and daughters, treating them with conspicuous
        courtesy — a propaganda move as well as a personal one.
        """,
        ["alexander-the-great", "darius-iii"],
    ),
    "battle-of-gaugamela": (
        "event",
        """
        At Gaugamela (1 October 331 BCE), on a leveled plain near modern
        Mosul, [[alexander-the-great]] decisively defeated [[darius-iii]]
        in the largest battle of the campaign. Outnumbered perhaps 2:1
        (the ancient figures of a million Persians are fantastic),
        Alexander's oblique advance forced a gap in the Persian line and
        led a cavalry charge directly at Darius, who again fled. The
        battle effectively ended the Achaemenid Empire; the Persian
        capitals Babylon, Susa, and Persepolis fell over the following
        year.
        """,
        ["alexander-the-great", "darius-iii"],
    ),
    "conquests-of-alexander": (
        "event",
        """
        Between 334 and 323 BCE, [[alexander-the-great|Alexander]] led the
        Macedonian-Greek army across the Hellespont, defeated [[darius-iii]]
        at Issus and Gaugamela, took Egypt and founded [[alexandria]], and
        pushed to the Indus. The campaigns ended the Achaemenid Empire,
        spread Greek language and institutions across western Asia, and
        opened the [[hellenistic-age]].
        """,
        ["alexander-the-great", "darius-iii", "alexandria", "hellenistic-age"],
    ),
}

GREECE_PLACES: dict[str, tuple[str, str, list[str]]] = {
    "athens": (
        "place",
        """
        Athens, capital of Attica, was the cultural and political center of
        classical Greece. Birthplace of [[democracy]] under [[solon]] and
        Cleisthenes, it produced [[pericles]], [[socrates]], [[plato]],
        [[aristotle]], the great tragedians, and an empire that lost the
        [[peloponnesian-war]]. The Acropolis with the Parthenon remains
        the city's defining monument.
        """,
        ["democracy", "solon", "pericles", "socrates", "plato", "aristotle", "peloponnesian-war"],
    ),
    "sparta": (
        "place",
        """
        Sparta, the dominant power of the Peloponnese, organized its entire
        society around the agoge — a state-run upbringing producing elite
        infantry. Its dual kingship and helot economy were singular in
        Greece. Sparta's victory in the [[peloponnesian-war]] under
        Lysander made it hegemon briefly; Theban [[battle-of-leuctra|Leuctra]]
        and then [[philip-ii|Macedonian]] Chaeronea ended that.
        """,
        ["peloponnesian-war", "battle-of-leuctra", "philip-ii"],
    ),
    "macedon": (
        "place",
        """
        Macedon was a northern Greek kingdom long regarded as semi-barbarian
        by southern poleis, transformed by [[philip-ii]] into the dominant
        military power of the Aegean and by [[alexander-the-great]] into a
        world empire stretching from the Adriatic to the Indus.
        """,
        ["philip-ii", "alexander-the-great"],
    ),
    "alexandria": (
        "place",
        """
        Alexandria, founded by [[alexander-the-great]] in 331 BCE, became
        under the [[ptolemy-i|Ptolemies]] the intellectual capital of the
        Hellenistic world. Its Library and Museum housed [[euclid]],
        Eratosthenes, and many later scholars. The city's last great
        royal patron was [[cleopatra]].
        """,
        ["alexander-the-great", "ptolemy-i", "euclid", "cleopatra"],
    ),
    "battle-of-leuctra": (
        "event",
        """
        At Leuctra (371 BCE), the Theban general Epaminondas crushed the
        [[sparta|Spartan]] army using an oblique phalanx, ending Spartan
        hegemony in Greece and beginning a brief Theban supremacy.
        """,
        ["sparta"],
    ),
    "darius-iii": (
        "person",
        """
        Darius III (c. 380–330 BCE), last Achaemenid king of Persia, lost
        Issus and Gaugamela to [[alexander-the-great]] and was murdered by
        his own satrap Bessus as Alexander pursued him into Bactria.
        """,
        ["alexander-the-great"],
    ),
    "cyrus-the-great": (
        "person",
        """
        Cyrus the Great (c. 600–530 BCE), founder of the Achaemenid Empire,
        was held up by [[xenophon]] in the Cyropaedia as the model of an
        enlightened ruler — a portrait that influenced Renaissance princes.
        """,
        ["xenophon"],
    ),
    "hellenistic-age": (
        "event",
        """
        The Hellenistic Age (323–31 BCE) is the period from
        [[alexander-the-great|Alexander's]] death to the Roman conquest of
        [[ptolemaic-egypt|Ptolemaic Egypt]]. Greek culture, language, and
        political forms spread from the Adriatic to the Indus; the
        successor kingdoms of the Diadochi — Antigonid, Seleucid, and
        Ptolemaic — fought and intermarried for three centuries until
        [[rome]] absorbed them piece by piece.
        """,
        ["alexander-the-great", "ptolemaic-egypt", "rome"],
    ),
}

GREECE_CONCEPTS: dict[str, tuple[str, str, list[str]]] = {
    "democracy": (
        "concept",
        """
        Democracy — *demos kratos*, rule of the people — emerged in
        [[athens]] under [[solon]] and Cleisthenes and was extended by
        [[pericles]]. Decisions were made in mass assemblies, juries were
        large and paid, offices were often filled by lot. [[plato]] and
        [[aristotle]] both criticized it; it was abolished after the
        [[peloponnesian-war]] and only briefly restored. The modern
        usage owes much more to Roman republicanism than to actual
        Athenian practice.
        """,
        ["athens", "solon", "pericles", "plato", "aristotle", "peloponnesian-war"],
    ),
    "polis": (
        "concept",
        """
        The polis — the self-governing city-state — was the basic political
        unit of classical Greece. [[athens]], [[sparta]], Corinth, Thebes:
        each was a polis. The era of the independent polis effectively
        ended when [[philip-ii]] won at Chaeronea and his son
        [[alexander-the-great]] absorbed Greece into a Macedonian empire.
        """,
        ["athens", "sparta", "philip-ii", "alexander-the-great"],
    ),
    "forms": (
        "concept",
        """
        Plato's theory of Forms holds that the changing particulars of
        sense experience are imperfect instances of eternal, unchanging
        intelligible universals — *the* Beautiful, *the* Just, *the* Good
        itself. Knowledge is recollection of these Forms. [[aristotle]]
        rejected the separation of universals from particulars; the
        dispute structures most of metaphysics through the Middle Ages.
        """,
        ["aristotle"],
    ),
    "cave": (
        "concept",
        """
        Plato's Allegory of the Cave, in book VII of the [[republic]],
        pictures most people as prisoners watching shadows on a wall and
        the philosopher as the one who has climbed out and seen the sun.
        It is the most famous metaphor in Western philosophy.
        """,
        ["republic"],
    ),
    "philosophy": (
        "concept",
        """
        Philosophy — *philo-sophia*, the love of wisdom — names the
        rational inquiry founded by the Presocratics and given its
        characteristic shape by [[socrates]], [[plato]], and [[aristotle]].
        The Hellenistic schools — [[stoicism]], [[epicureanism]],
        [[cynicism]], skepticism — turned the inquiry toward life-practice.
        """,
        ["socrates", "plato", "aristotle", "stoicism", "epicureanism", "cynicism"],
    ),
    "stoicism": (
        "concept",
        """
        Stoicism, founded by [[zeno-of-citium]] in [[athens]], teaches that
        virtue is the only good, the cosmos is ordered by divine reason
        (logos), and emotions are mistaken judgments to be corrected by
        practice. Its great Roman exponents were [[seneca]], [[epictetus]],
        and [[marcus-aurelius]]. It has had a substantial popular revival
        in the 21st century.
        """,
        ["zeno-of-citium", "athens", "seneca", "epictetus", "marcus-aurelius"],
    ),
    "epicureanism": (
        "concept",
        """
        Epicureanism, founded by [[epicurus]], identifies pleasure — defined
        as the absence of bodily pain and mental disturbance — as the
        highest good. Its physics is atomist; its theology deistic. The
        Roman poet [[lucretius]] gave it its most influential literary
        expression in [[de-rerum-natura]].
        """,
        ["epicurus", "lucretius", "de-rerum-natura"],
    ),
    "cynicism": (
        "concept",
        """
        Cynicism, founded by [[diogenes]] and Antisthenes, taught that
        virtue is lived in accord with nature, in defiance of social
        convention, money, power, and fame. The Cynic was the gadfly of
        the polis. The school's influence on [[stoicism]] was direct.
        """,
        ["diogenes", "stoicism"],
    ),
    "hoplite-phalanx": (
        "concept",
        """
        The hoplite phalanx — heavy-infantry citizen-soldiers fighting
        shoulder-to-shoulder in an eight-deep rectangular formation with
        round aspis shields, eight-foot spears, and bronze helmets —
        was the characteristic military formation of classical Greece.
        It rewarded mass discipline over individual heroics; the citizen
        who could afford the panoply (hoplon) fought, anchoring the
        political claim to participate in the [[polis]]. [[philip-ii]]
        broke its dominance with the longer Macedonian sarissa.
        """,
        ["polis", "philip-ii"],
    ),
    "helot": (
        "concept",
        """
        The helots were the state-owned serf population of Lakonia and
        Messenia who farmed the land for [[sparta|Spartan]] citizens
        (Spartiates). They outnumbered their masters perhaps 7-to-1;
        Spartan society's distinctive militarism is best understood as
        the apparatus required to keep them subjugated. Each year the
        ephors ritually declared war on them so any Spartiate could kill
        a helot without religious pollution. Helots fought in war as
        light troops and were emancipated en masse only after
        [[battle-of-leuctra|Leuctra]] broke Spartan power.
        """,
        ["sparta", "battle-of-leuctra"],
    ),
    "ostracism": (
        "concept",
        """
        Ostracism was an [[athens|Athenian]] democratic procedure,
        traditionally credited to [[cleisthenes]], by which once a year
        the assembly could vote to exile any citizen for ten years
        without trial. Voters scratched the name on a potsherd
        (ostrakon — hundreds survive); if 6,000 votes were cast, the
        man with the most was ostracized. [[themistocles]] was ostracized
        after Salamis. The practice fell out of use after 417 BCE.
        """,
        ["athens", "cleisthenes", "themistocles"],
    ),
    "solon-reforms": (
        "concept",
        """
        Solon's reforms (594/3 BCE) were the constitutional package
        [[solon]] enacted as archon to defuse a debt crisis that
        threatened civil war in [[athens]]. He cancelled outstanding
        debts (the *seisachtheia*, "shaking-off-of-burdens"), abolished
        debt-slavery, replaced birth-based political eligibility with a
        four-class division by income measured in agricultural produce
        (pentakosiomedimnoi, hippeis, zeugitai, thetes), and created or
        reformed the Council of 400 and the popular law-courts. His
        framework — neither democracy nor pure aristocracy — laid the
        ground that [[cleisthenes]] would push further toward
        [[democracy]] a century later.
        """,
        ["solon", "athens", "cleisthenes", "democracy"],
    ),
}

# ─────────────────────────────────────────────────────────────────────────
# ROME
# ─────────────────────────────────────────────────────────────────────────

ROME_PEOPLE: dict[str, tuple[str, str, list[str]]] = {
    "romulus-and-remus": (
        "person",
        """
        Romulus and Remus, the legendary twin founders of [[rome]], were
        sons of Mars and the Vestal Virgin Rhea Silvia. Suckled by a
        she-wolf after being exposed by a usurping uncle, they grew up to
        found a city on the Palatine in 753 BCE — at least by Roman
        reckoning. Romulus killed Remus in a quarrel over the city's
        walls and became its first king. The story is told by [[livy]] in
        the [[history-of-rome]].
        """,
        ["rome", "livy", "history-of-rome"],
    ),
    "julius-caesar": (
        "person",
        """
        Gaius Julius Caesar (12 or 13 July 100 BCE – 15 March 44 BCE), of
        the patrician gens Julia, rose through a conventional senatorial
        cursus honorum (quaestor 69, aedile 65, pontifex maximus 63,
        praetor 62, consul 59 BCE). His political alliance with [[pompey]]
        and [[crassus]] — the First Triumvirate, formalized 60 BCE,
        cemented by Pompey's marriage to Caesar's daughter Julia —
        secured him the proconsular command in Gaul. Over 58–50 BCE the
        [[gallic-wars]] subjugated all of Gaul and twice crossed the
        Rhine and the Channel; his *Commentarii de Bello Gallico* (third
        person, deliberately spare) made him a literary as well as
        military figure. Julia's death in 54 and Crassus' death at
        [[battle-of-carrhae|Carrhae]] in 53 BCE dissolved the triumvirate.
        When the Senate, backed by Pompey, ordered him to disband his
        army, he crossed the Rubicon (10–11 January 49 BCE) with Legio
        XIII, reportedly saying *alea iacta est* — "the die is cast." He
        defeated Pompey at [[battle-of-pharsalus|Pharsalus]] in Thessaly
        (48 BCE), pursued him to Egypt where Pompey was murdered by
        Ptolemy XIII's courtiers, intervened in the Egyptian civil war on
        [[cleopatra|Cleopatra's]] side, defeated Pharnaces II at Zela in
        47 (*veni, vidi, vici*), crushed the Pompeian remnant at Thapsus
        (46) and Munda (45). Named dictator perpetuo in February 44 BCE,
        he was stabbed to death twenty-three times in the Theatre of
        Pompey on the [[ides-of-march]] by a conspiracy of some sixty
        senators led by Marcus Junius Brutus and Gaius Cassius Longinus.
        He had named his great-nephew [[augustus|Octavian]] as principal
        heir and adopted son in his will; that adoption made the empire
        possible. Principal sources: Caesar's own commentaries,
        [[suetonius]], [[plutarch]], [[cicero|Cicero's]] letters, the
        anonymous *Bellum Alexandrinum* and *Bellum Africum*.
        """,
        ["pompey", "crassus", "gallic-wars", "battle-of-carrhae", "battle-of-pharsalus", "cleopatra", "ides-of-march", "augustus", "suetonius", "plutarch", "cicero"],
    ),
    "augustus": (
        "person",
        """
        Augustus (Gaius Octavius, 23 September 63 BCE – 19 August 14 CE),
        first Roman emperor, was the grand-nephew of [[julius-caesar]] and
        adopted as his son by Caesar's will. At nineteen, in 44 BCE, he
        used Caesar's name (rebranding himself Gaius Julius Caesar
        Octavianus — "Octavian" is a modern convention) and Caesar's
        veterans to push his way into politics over [[cicero|Cicero's]]
        objections. He joined [[mark-antony]] and Lepidus in the Second
        Triumvirate (43 BCE), legally constituted by the lex Titia — under
        whose proscriptions Cicero was murdered. He and Antony defeated
        Brutus and Cassius at [[battle-of-philippi|Philippi]] (42 BCE),
        then divided the Roman world: Octavian taking the West, Antony
        the East. Their alliance — sealed by Antony's marriage to
        Octavia, Octavian's sister — frayed as Antony tied himself ever
        more closely to [[cleopatra]] of [[ptolemaic-egypt|Egypt]]. After
        the publicity coup of reading Antony's will (real or fabricated)
        and a propaganda war framing the coming conflict as a struggle
        against an eastern queen, his fleet under Agrippa won the
        [[battle-of-actium]] (2 September 31 BCE). Antony and Cleopatra
        committed suicide in Alexandria within the year. From 27 BCE he
        styled himself *princeps* — "first citizen" — and the Senate
        voted him the new name Augustus ("revered"). The constitutional
        settlements of 27 and 23 BCE created the principate without
        formally abolishing the Republic — magistracies and Senate
        continued; real power flowed from his proconsular *imperium*,
        tribunician *potestas*, control of the legions, and immense
        personal wealth. His reign (27 BCE – 14 CE) inaugurated the
        [[pax-romana]], reorganized the provinces, professionalized the
        army with fixed terms and pensions, established the Praetorian
        Guard, patronized [[virgil]] (commissioning the [[aeneid]]),
        [[horace]], [[ovid]] (whom he later exiled), and [[livy]]. He
        survived three named heirs (Marcellus, Agrippa, his grandsons
        Gaius and Lucius) before settling on his stepson Tiberius. His
        own account is the [[res-gestae|Res Gestae Divi Augusti]],
        inscribed at his mausoleum and on copies across the empire.
        """,
        ["julius-caesar", "cicero", "mark-antony", "battle-of-philippi", "cleopatra", "ptolemaic-egypt", "battle-of-actium", "pax-romana", "virgil", "aeneid", "horace", "ovid", "livy", "res-gestae"],
    ),
    "mark-antony": (
        "person",
        """
        Mark Antony (83–30 BCE) was [[julius-caesar|Caesar's]] lieutenant,
        co-triumvir with [[augustus|Octavian]] and Lepidus after Caesar's
        assassination on the [[ides-of-march]], and lover of [[cleopatra]].
        Defeated at the [[battle-of-actium]], he and Cleopatra committed
        suicide in Alexandria in 30 BCE.
        """,
        ["julius-caesar", "augustus", "ides-of-march", "cleopatra", "battle-of-actium"],
    ),
    "cleopatra": (
        "person",
        """
        Cleopatra VII Philopator (69–30 BCE) was the last effective ruler
        of the [[ptolemaic-egypt|Ptolemaic kingdom]] of Egypt and the
        last Hellenistic monarch. Daughter of Ptolemy XII Auletes, she
        was reportedly the only Ptolemaic ruler in three centuries to
        learn Egyptian as well as her native Greek (and, [[plutarch]]
        says, eight or nine other languages). She ruled jointly with two
        younger brother-husbands (Ptolemy XIII and XIV) and her son
        Ptolemy XV Caesarion. Forced from [[alexandria]] in a civil war
        with Ptolemy XIII's faction, she was famously smuggled to
        [[julius-caesar]] (probably rolled in bedding, not a carpet —
        Plutarch's word is *stromatodesmon*) when he wintered in Egypt
        in 48–47 BCE pursuing [[pompey]]. Caesar restored her to the
        throne, fathered Caesarion, and brought her to [[rome]] for two
        years before his assassination on the [[ides-of-march]]. She
        allied next with [[mark-antony]]: they met at Tarsus in 41 BCE,
        had three children (Alexander Helios, Cleopatra Selene II,
        Ptolemy Philadelphus), and at the "Donations of Alexandria" (34
        BCE) Antony distributed Roman eastern provinces to her and her
        children as kingdoms — propaganda gold for [[augustus|Octavian]].
        After defeat at the [[battle-of-actium]] (31 BCE), Antony killed
        himself on a false report of her death; she followed days or
        weeks later, in [[plutarch|Plutarch's]] account by the bite of
        an asp, in modern scholarship's by some other means. Egypt was
        annexed as a Roman imperial province under direct equestrian
        rule, the first such arrangement.
        """,
        ["ptolemaic-egypt", "plutarch", "alexandria", "julius-caesar", "pompey", "rome", "ides-of-march", "mark-antony", "augustus", "battle-of-actium"],
    ),
    "cicero": (
        "person",
        """
        Marcus Tullius Cicero (106–43 BCE), Roman statesman and orator,
        prosecuted Catiline's conspiracy as consul, opposed [[mark-antony]]
        in the Philippics, and was proscribed and killed by the
        triumvirate. His prose set the standard for Latin and his
        philosophical works carried Greek [[stoicism]] and skepticism into
        Roman thought. [[augustus]] later called him a "learned man and a
        patriot."
        """,
        ["mark-antony", "stoicism", "augustus"],
    ),
    "pompey": (
        "person",
        """
        Gnaeus Pompeius Magnus (106–48 BCE), "Pompey the Great," cleared
        the Mediterranean of pirates, settled the East, and formed the
        First Triumvirate with [[julius-caesar]] and [[crassus]]. After
        Crassus' death he opposed Caesar in the civil war, lost at
        Pharsalus (48 BCE), and was assassinated landing in Egypt.
        """,
        ["julius-caesar", "crassus"],
    ),
    "crassus": (
        "person",
        """
        Marcus Licinius Crassus (115–53 BCE), the richest man in [[rome]],
        crushed [[spartacus|Spartacus']] slave revolt and formed the First
        Triumvirate with [[julius-caesar]] and [[pompey]]. Killed at
        Carrhae in 53 BCE pursuing eastern conquest, his death destabilized
        the triumvirate and helped trigger the civil war.
        """,
        ["rome", "spartacus", "julius-caesar", "pompey"],
    ),
    "spartacus": (
        "person",
        """
        Spartacus (died 71 BCE) was a Thracian gladiator who led the largest
        slave revolt of the Roman Republic, defeating several Roman armies
        before [[crassus]] crushed his forces in southern Italy. Six
        thousand survivors were crucified along the Appian Way.
        """,
        ["crassus"],
    ),
    "hannibal": (
        "person",
        """
        Hannibal Barca (247–c. 183/181 BCE), Carthaginian general, son of
        Hamilcar Barca (commander of [[carthage|Carthaginian]] forces in
        Sicily during the [[first-punic-war]]) and brother of Hasdrubal
        Barca and Mago. As a boy he reportedly swore on an altar an
        oath of eternal enmity to [[rome]]. He took command of the
        Carthaginian army in Spain in 221 BCE at twenty-six and provoked
        the [[second-punic-war]] in 218 BCE by besieging Saguntum. His
        crossing of the Alps in autumn 218 — with somewhere between 38
        and 60 elephants, of which only one (Surus) survived the first
        winter — brought him into Italy at the head of perhaps 26,000
        troops. He crushed Roman armies at the Trebia (218), Lake
        Trasimene (217, where the consul Flaminius was killed and three
        legions destroyed in an ambush), and most spectacularly at
        [[battle-of-cannae|Cannae]] (2 August 216 BCE), where he
        enveloped and destroyed a Roman force of roughly 80,000 — perhaps
        the most studied tactical victory in Western military history.
        Rome lost an estimated 50,000–70,000 men in a single day; the
        gold rings of the dead equestrians filled, [[livy]] says, three
        bushels. Yet Rome refused to negotiate. Fabius Maximus' delaying
        strategy ("Cunctator") denied Hannibal pitched battle; his brother
        Hasdrubal was killed at the Metaurus (207) before he could
        reinforce; and [[scipio-africanus|Scipio's]] invasion of Africa
        forced Hannibal home, where Scipio defeated him at
        [[battle-of-zama|Zama]] in 202 BCE. After the war he served as
        Carthaginian suffete and reformer until Roman pressure drove him
        into exile at the court of the Seleucid Antiochus III, then
        Prusias I of Bithynia. He took poison around 183 BCE rather than
        be handed over to Roman extradition. Principal sources: Polybius,
        Livy books 21–30, Plutarch's Fabius and Marcellus, Appian.
        """,
        ["carthage", "first-punic-war", "rome", "second-punic-war", "battle-of-cannae", "livy", "scipio-africanus", "battle-of-zama"],
    ),
    "scipio-africanus": (
        "person",
        """
        Publius Cornelius Scipio Africanus (236–183 BCE) was the Roman
        general who defeated [[hannibal]] at Zama in 202 BCE, ending the
        [[second-punic-war]]. His Spanish campaigns and his sponsorship of
        Polybius made him a model of the Hellenized Roman aristocrat.
        """,
        ["hannibal", "second-punic-war"],
    ),
    "scipio-aemilianus": (
        "person",
        """
        Scipio Aemilianus (185–129 BCE), grandson by adoption of
        [[scipio-africanus]], destroyed [[carthage]] in the [[third-punic-war]]
        (146 BCE) and reputedly wept while quoting [[homer]] over the
        burning city, foreseeing Rome's own eventual fall.
        """,
        ["scipio-africanus", "carthage", "third-punic-war", "homer"],
    ),
    "marcus-aurelius": (
        "person",
        """
        Marcus Aurelius (121–180 CE), Roman emperor and Stoic philosopher,
        wrote the *Meditations* — a private notebook of [[stoicism|Stoic]]
        self-exhortation that survives as one of the great works of
        practical philosophy. He campaigned on the Danube frontier against
        Germanic tribes and was succeeded, disastrously, by his son
        [[commodus]].
        """,
        ["stoicism", "commodus"],
    ),
    "commodus": (
        "person",
        """
        Commodus (161–192 CE), son of [[marcus-aurelius]], reigned as a
        capricious tyrant who fought as a gladiator and was eventually
        strangled in his bath. His reign is conventionally taken to mark
        the end of the [[pax-romana]] and the start of Rome's long decline.
        """,
        ["marcus-aurelius", "pax-romana"],
    ),
    "nero": (
        "person",
        """
        Nero (37–68 CE) was the fifth Roman emperor, remembered for the
        Great Fire of [[rome]] (which contemporaries already accused him
        of starting), the persecution of Christians, and the murder of his
        mother Agrippina. He took his own life as the legions revolted,
        ending the Julio-Claudian dynasty.
        """,
        ["rome"],
    ),
    "caligula": (
        "person",
        """
        Caligula (12–41 CE), third Roman emperor, began competently and
        descended into cruelty and incoherence — making his horse a
        consul, declaring himself a god. He was assassinated by his own
        Praetorian Guard.
        """,
        [],
    ),
    "constantine": (
        "person",
        """
        Constantine I (c. 272–337 CE), Roman emperor, legalized Christianity
        with the Edict of Milan (313), called the Council of Nicaea (325),
        and refounded [[byzantium|Byzantium]] as Constantinople in 330 —
        beginning the long history of the [[byzantine-empire]].
        """,
        ["byzantium", "byzantine-empire"],
    ),
    "trajan": (
        "person",
        """
        Trajan (53–117 CE), Roman emperor, expanded the empire to its
        greatest territorial extent with the conquest of Dacia (modern
        Romania) and brief campaigns in Mesopotamia. His column in
        [[rome]] is the great surviving sculptural narrative of a Roman
        war.
        """,
        ["rome"],
    ),
    "hadrian": (
        "person",
        """
        Hadrian (76–138 CE), successor to [[trajan]], consolidated rather
        than expanded — pulling back from Mesopotamia and building the
        wall in northern Britain that still bears his name. He was a
        traveler, Hellenophile, and builder; the Pantheon in [[rome]] is
        his.
        """,
        ["trajan", "rome"],
    ),
    "diocletian": (
        "person",
        """
        Diocletian (244–311 CE) ended the third-century crisis by
        reorganizing the empire under the Tetrarchy — two senior
        emperors (Augusti) and two junior (Caesars). His reforms shaped
        the late-Roman bureaucratic state and set the stage for
        [[constantine]]. He launched the last and most severe pagan
        persecution of Christians (303–311). Uniquely among Roman
        emperors he voluntarily abdicated (305 CE) and retired to his
        palace at Split, where he reportedly told a visitor urging him
        back to power to come see the cabbages he was growing.
        """,
        ["constantine"],
    ),
    "tiberius": (
        "person",
        """
        Tiberius (42 BCE – 37 CE), stepson and successor of [[augustus]],
        second Roman emperor. Able general and reluctant ruler, he
        retired to Capri in 26 CE leaving the empire largely to his
        Praetorian prefect Sejanus, whose conspiracy he eventually
        crushed in 31. Tacitus and Suetonius painted him in increasingly
        dark colors; the historical Tiberius was probably gloomier and
        more competent than the monster of [[tacitus|Tacitus']] [[annals]].
        """,
        ["augustus", "tacitus", "annals"],
    ),
    "vespasian": (
        "person",
        """
        Vespasian (9–79 CE), Roman emperor, founder of the Flavian
        dynasty. A successful general in Britain and Judaea (where he
        was suppressing the First Jewish Revolt when proclaimed
        emperor), he emerged as the winner of the Year of the Four
        Emperors (69 CE). Restored stability after [[nero|Nero's]] chaos,
        began the Colosseum, and is famous for the dry remark on his
        deathbed: *vae, puto deus fio* — "alas, I think I am becoming
        a god." His son [[titus]] completed the Jewish War with the
        sack of Jerusalem in 70 CE.
        """,
        ["nero", "titus"],
    ),
    "titus": (
        "person",
        """
        Titus (39–81 CE), Roman emperor, son of [[vespasian]]. As his
        father's general he ended the First Jewish Revolt by sacking
        Jerusalem and destroying the Second Temple in 70 CE (commemorated
        on the Arch of Titus in [[rome]]). His brief reign as emperor
        saw the eruption of Vesuvius (79 CE, burying Pompeii and
        Herculaneum), a great fire in Rome, and the inauguration of the
        Colosseum. Died of fever after two years.
        """,
        ["vespasian", "rome"],
    ),
    "domitian": (
        "person",
        """
        Domitian (51–96 CE), younger son of [[vespasian]] and last Flavian
        emperor. Capable administrator, increasingly autocratic and
        paranoid; insisted on being addressed as *dominus et deus*
        ("lord and god"). Murdered in a palace conspiracy that included
        his own wife; the Senate decreed damnatio memoriae and gleefully
        appointed the elderly senator Nerva, beginning the line of the
        [[five-good-emperors|Five Good Emperors]].
        """,
        ["vespasian", "five-good-emperors"],
    ),
    "nerva": (
        "person",
        """
        Nerva (c. 30–98 CE), elderly senator chosen by the Senate to
        succeed [[domitian]] in 96 CE. His sixteen-month reign mattered
        chiefly for one act: adopting [[trajan]] as his heir, beginning
        the practice of selecting the best available successor that
        defined the [[five-good-emperors|Five Good Emperors]] from him
        to [[marcus-aurelius]].
        """,
        ["domitian", "trajan", "five-good-emperors", "marcus-aurelius"],
    ),
    "antoninus-pius": (
        "person",
        """
        Antoninus Pius (86–161 CE), Roman emperor, adopted successor of
        [[hadrian]] and predecessor of [[marcus-aurelius]]. His
        twenty-three-year reign is the quietest of the [[five-good-emperors|Five
        Good Emperors]] — almost no narrative survives because almost
        nothing went wrong. The high noon of the [[pax-romana]].
        """,
        ["hadrian", "marcus-aurelius", "five-good-emperors", "pax-romana"],
    ),
    "caracalla": (
        "person",
        """
        Caracalla (188–217 CE), Roman emperor of the Severan dynasty,
        murdered his brother Geta in their mother's arms to secure sole
        rule, and in 212 CE issued the Constitutio Antoniniana extending
        Roman citizenship to nearly all free inhabitants of the empire
        — the largest single expansion of citizenship in human history,
        partly motivated by extending the inheritance tax base. He was
        assassinated by a soldier on campaign against Parthia.
        """,
        [],
    ),
    "theodosius": (
        "person",
        """
        Theodosius I (347–395 CE) was the last emperor to rule both
        eastern and western halves of the Roman Empire. He made Nicene
        Christianity the state religion by the Edict of Thessalonica
        (380), suppressed pagan worship and closed the temples (including
        the [[athens|Athenian]] Eleusinian Mysteries and the Olympic
        Games), and on his death partitioned the empire between his sons
        Arcadius (East) and Honorius (West) — a division that proved
        permanent.
        """,
        ["athens"],
    ),
    "julian-the-apostate": (
        "person",
        """
        Julian (331–363 CE), Roman emperor and the last pagan to hold
        the throne, nephew of [[constantine]]. Raised Christian but
        privately converted to Hellenic paganism, he attempted on
        accession (361) to restore the old religion, reopen the
        temples, and curb the Christian church — including reportedly
        a project to rebuild the Jerusalem Temple. Killed by a Persian
        spear after eighteen months, on a Persian campaign that recalled
        [[alexander-the-great|Alexander's]] but ended like
        [[crassus|Crassus']].
        """,
        ["constantine", "alexander-the-great", "crassus"],
    ),
}

ROME_WRITERS: dict[str, tuple[str, str, list[str]]] = {
    "virgil": (
        "person",
        """
        Publius Vergilius Maro (70–19 BCE), Rome's national poet, wrote the
        *Eclogues*, the *Georgics*, and the [[aeneid]] — the epic he left
        unfinished at his death. The Aeneid traces [[aeneas]] from the
        fall of Troy to Italy, founding the line that will lead to
        [[romulus-and-remus]] and ultimately to [[augustus]], the poem's
        patron.
        """,
        ["aeneid", "aeneas", "romulus-and-remus", "augustus"],
    ),
    "ovid": (
        "person",
        """
        Publius Ovidius Naso (43 BCE – 17/18 CE) wrote the [[metamorphoses]],
        the *Ars Amatoria*, the *Heroides*, and the *Tristia* — the last
        from exile on the Black Sea, where [[augustus]] banished him for
        reasons never fully explained. The Metamorphoses is the most
        important single source of Greco-Roman mythology for the European
        tradition after antiquity.
        """,
        ["metamorphoses", "augustus"],
    ),
    "horace": (
        "person",
        """
        Quintus Horatius Flaccus (65–8 BCE), Roman lyric poet under
        [[augustus]], wrote the *Odes*, the *Satires*, and the *Epistles*
        (including the Ars Poetica). *Carpe diem* and *aurea mediocritas*
        are his coinages, and his diction influenced everyone in Latin
        verse after him.
        """,
        ["augustus"],
    ),
    "livy": (
        "person",
        """
        Titus Livius (59 BCE – 17 CE) wrote the [[history-of-rome]] *Ab
        Urbe Condita* — from the founding of the city through his own
        time. Only 35 of the original 142 books survive. He is the
        principal narrative source for the early Republic, including
        [[romulus-and-remus]], the kings, the [[punic-wars]], and the
        struggle of the orders.
        """,
        ["history-of-rome", "romulus-and-remus", "punic-wars"],
    ),
    "tacitus": (
        "person",
        """
        Publius Cornelius Tacitus (c. 56 – c. 120 CE) wrote the [[annals]]
        and the *Histories* — astringent, morally severe narratives of the
        Julio-Claudian and Flavian dynasties, including [[nero]] and the
        civil war of 69 CE. His *Germania* is a major source on early
        Germanic peoples.
        """,
        ["annals", "nero"],
    ),
    "suetonius": (
        "person",
        """
        Gaius Suetonius Tranquillus (c. 69 – after 122 CE) wrote *The
        Twelve Caesars*, biographies from [[julius-caesar]] through
        Domitian. Gossipy, sometimes lurid, indispensable: most of what
        modern readers "know" about [[nero]] or [[caligula]] is Suetonius.
        """,
        ["julius-caesar", "nero", "caligula"],
    ),
    "catullus": (
        "person",
        """
        Gaius Valerius Catullus (c. 84–54 BCE) was the foremost of the
        "neoteric" poets of the late Republic. His short poems to and
        about Lesbia (probably Clodia Metelli) are the most personal
        voice in Latin verse — by turns ecstatic, vindictive, and
        miserable.
        """,
        [],
    ),
    "lucretius": (
        "person",
        """
        Titus Lucretius Carus (c. 99–55 BCE) wrote [[de-rerum-natura]] —
        *On the Nature of Things* — a six-book Latin hexameter exposition
        of [[epicureanism|Epicurean]] physics and ethics. Its rediscovery
        in 1417 by Poggio Bracciolini is sometimes credited as a spark of
        the Renaissance.
        """,
        ["de-rerum-natura", "epicureanism"],
    ),
    "seneca": (
        "person",
        """
        Lucius Annaeus Seneca (c. 4 BCE – 65 CE), Stoic philosopher,
        playwright, and tutor-then-adviser to [[nero]], was eventually
        compelled by the emperor to commit suicide. His *Letters to
        Lucilius* are the most accessible introduction to Roman
        [[stoicism]].
        """,
        ["nero", "stoicism"],
    ),
}

ROME_WORKS: dict[str, tuple[str, str, list[str]]] = {
    "aeneid": (
        "work",
        """
        [[virgil|Virgil's]] Aeneid is the Roman national epic, narrating
        [[aeneas|Aeneas']] flight from burning Troy, his affair with
        Carthaginian Dido (the mythical proto-foundation of the enmity
        with [[carthage]]), his descent to the underworld, and his
        founding of the Italian line that would produce
        [[romulus-and-remus]] and, the poem implies, [[augustus]] himself.
        """,
        ["virgil", "aeneas", "carthage", "romulus-and-remus", "augustus"],
    ),
    "metamorphoses": (
        "work",
        """
        [[ovid|Ovid's]] Metamorphoses, fifteen books of Latin hexameter,
        retells some 250 myths of transformation from the creation of the
        world through the apotheosis of [[julius-caesar]]. It is the
        principal conduit through which Greco-Roman myth reached medieval
        and Renaissance Europe.
        """,
        ["ovid", "julius-caesar"],
    ),
    "history-of-rome": (
        "work",
        """
        [[livy|Livy's]] Ab Urbe Condita Libri was a 142-book history of
        [[rome]] from the legendary founding to the early Augustan age.
        Only books 1–10 and 21–45 survive entire. The early books are
        the canonical narrative of [[romulus-and-remus]], the kings,
        Lucretia, Cincinnatus, and Camillus.
        """,
        ["livy", "rome", "romulus-and-remus"],
    ),
    "annals": (
        "work",
        """
        [[tacitus|Tacitus']] Annals covers the Julio-Claudian emperors from
        Tiberius through [[nero]]. His epigrammatic Latin and barely-veiled
        republican sympathies make him the most quoted ancient historian
        in modern political writing.
        """,
        ["tacitus", "nero"],
    ),
    "de-rerum-natura": (
        "work",
        """
        [[lucretius|Lucretius']] De Rerum Natura — *On the Nature of
        Things* — sets [[epicureanism|Epicurean]] philosophy to Latin
        hexameter in six books, covering atoms, the soul, the senses,
        cosmology, and the origins of civilization. Its denial of divine
        providence and personal immortality made it dangerous reading
        through the Christian centuries.
        """,
        ["lucretius", "epicureanism"],
    ),
    "res-gestae": (
        "work",
        """
        The Res Gestae Divi Augusti — "Achievements of the Divine
        Augustus" — is the first-person summary of [[augustus|Augustus']]
        career that he composed late in life and ordered inscribed on
        bronze pillars at his mausoleum in [[rome]]. Provincial copies in
        Latin and Greek survive, most completely on the walls of the
        Temple of Augustus and Roma at Ancyra (modern Ankara). It is the
        most important political autobiography from antiquity — every
        omission as informative as every claim.
        """,
        ["augustus", "rome"],
    ),
    "meditations": (
        "work",
        """
        The Meditations of [[marcus-aurelius]], written in Greek under
        the title *Ta eis heauton* — "to himself" — across the late
        years of his reign while on campaign on the Danube, is a
        private notebook of [[stoicism|Stoic]] self-exhortation never
        intended for publication. Twelve short books of disconnected
        entries, repetitive and meditative, on death, duty, the
        cosmos, and the rule of reason. The most-read work of practical
        philosophy from antiquity.
        """,
        ["marcus-aurelius", "stoicism"],
    ),
}

ROME_EVENTS: dict[str, tuple[str, str, list[str]]] = {
    "founding-of-rome": (
        "event",
        """
        Tradition dates the founding of [[rome]] to 753 BCE by
        [[romulus-and-remus]]. The early period of kings ended in 509 BCE
        with the expulsion of Tarquinius Superbus and the founding of the
        Republic — the system that would govern Rome through the
        [[punic-wars]] and down to [[julius-caesar]].
        """,
        ["rome", "romulus-and-remus", "punic-wars", "julius-caesar"],
    ),
    "punic-wars": (
        "event",
        """
        The three Punic Wars (264–146 BCE) between [[rome]] and [[carthage]]
        determined which city would dominate the western Mediterranean.
        The [[first-punic-war]] won Sicily; the [[second-punic-war]],
        marked by [[hannibal|Hannibal's]] invasion of Italy, nearly ended
        Rome but ended Carthaginian sea-power; the [[third-punic-war]]
        wiped Carthage from the map.
        """,
        ["rome", "carthage", "first-punic-war", "second-punic-war", "third-punic-war", "hannibal"],
    ),
    "first-punic-war": (
        "event",
        """
        The First Punic War (264–241 BCE) was fought primarily at sea over
        Sicily. [[rome|Rome]] built its first real navy and ultimately
        defeated [[carthage]], gaining Sicily as its first overseas
        province.
        """,
        ["rome", "carthage"],
    ),
    "second-punic-war": (
        "event",
        """
        The Second Punic War (218–201 BCE) is the [[hannibal|Hannibalic]] war:
        elephants over the Alps, Cannae, Fabius the Delayer, and finally
        [[scipio-africanus|Scipio's]] invasion of Africa and victory at
        Zama. [[archimedes]] died in its margins, defending Syracuse.
        """,
        ["hannibal", "scipio-africanus", "archimedes", "battle-of-cannae"],
    ),
    "third-punic-war": (
        "event",
        """
        The Third Punic War (149–146 BCE) ended with [[scipio-aemilianus|Scipio
        Aemilianus]] razing [[carthage]] to the ground. Cato the Elder
        had ended every senate speech with *Carthago delenda est* — and
        finally got his wish.
        """,
        ["scipio-aemilianus", "carthage"],
    ),
    "battle-of-cannae": (
        "event",
        """
        At Cannae (2 August 216 BCE), [[hannibal]] enveloped and
        destroyed a Roman army of roughly 80,000 men under the consuls
        Varro and Aemilius Paullus — perhaps 50,000–70,000 killed in a
        single afternoon. Hannibal weakened his center deliberately,
        letting it bow inward as the Romans pressed forward, while his
        Spanish and African heavy infantry held the flanks and his
        Numidian and Spanish cavalry routed the Roman horse and circled
        behind. The Romans were enveloped on all sides and slaughtered.
        Still taught in military academies as the model of double
        envelopment; Schlieffen's 1905 plan against France was an
        explicit attempt to repeat it.
        """,
        ["hannibal"],
    ),
    "battle-of-zama": (
        "event",
        """
        At Zama (October 202 BCE), [[scipio-africanus]] defeated
        [[hannibal]] in North Africa, ending the [[second-punic-war]].
        Scipio had studied Hannibal's tactics; he opened lanes in his
        infantry to let Hannibal's elephants pass through harmlessly,
        and his returning Numidian cavalry under Masinissa repeated
        against Hannibal what Hannibal's cavalry had done at
        [[battle-of-cannae|Cannae]] — falling on the enemy infantry
        from behind. Carthage sued for peace.
        """,
        ["scipio-africanus", "hannibal", "second-punic-war", "battle-of-cannae"],
    ),
    "battle-of-carrhae": (
        "event",
        """
        At Carrhae (June 53 BCE), [[crassus]] led seven legions deep
        into Parthian territory in pursuit of glory to match
        [[julius-caesar|Caesar's]] in Gaul. The Parthian general Surena,
        commanding mounted archers supplied by an enormous baggage train
        of arrows, encircled and shot the Romans to pieces in the open
        desert. Crassus' son Publius was killed, then Crassus himself
        under flag of truce; molten gold was reportedly poured down his
        throat in mockery of his greed. The defeat destabilized the
        [[fall-of-the-republic|First Triumvirate]] and lost seven legionary
        eagles that [[augustus]] later negotiated back as the
        propaganda centerpiece of his Parthian settlement.
        """,
        ["crassus", "julius-caesar", "fall-of-the-republic", "augustus"],
    ),
    "battle-of-pharsalus": (
        "event",
        """
        At Pharsalus in Thessaly (9 August 48 BCE), [[julius-caesar]]
        defeated [[pompey]] in the decisive battle of their civil war.
        Outnumbered roughly 22,000 to 45,000, Caesar held a fourth line
        of reserves who, instead of throwing pila at Pompey's cavalry,
        thrust them at the riders' faces — routing the cavalry and
        exposing Pompey's left flank. Pompey fled to Egypt where Ptolemy
        XIII's courtiers murdered him on the beach to curry favor with
        the victor.
        """,
        ["julius-caesar", "pompey"],
    ),
    "battle-of-philippi": (
        "event",
        """
        At Philippi in Macedonia (October 42 BCE), the armies of the
        Second Triumvirate — [[augustus|Octavian]] and [[mark-antony]] —
        defeated the Liberators Brutus and Cassius in two engagements.
        Cassius killed himself after the first, mistakenly believing
        Brutus had been defeated; Brutus killed himself after the
        second. The battle ended the republican cause; thereafter the
        only question was which Caesarian faction would rule.
        """,
        ["augustus", "mark-antony"],
    ),
    "gallic-wars": (
        "event",
        """
        The Gallic Wars (58–50 BCE) were [[julius-caesar|Caesar's]] eight
        campaigns conquering Gaul (modern France, Belgium, and parts of
        the Rhineland). His commentaries are the principal source. Victory
        gave him wealth, an army loyal to him personally, and the political
        weight to challenge [[pompey]].
        """,
        ["julius-caesar", "pompey"],
    ),
    "ides-of-march": (
        "event",
        """
        On the Ides of March (15 March 44 BCE), [[julius-caesar]] was
        assassinated in the Senate by a conspiracy of senators led by
        Brutus and Cassius. The assassination did not save the Republic
        — it triggered a second civil war that ended with [[augustus]]
        as the first emperor.
        """,
        ["julius-caesar", "augustus"],
    ),
    "battle-of-actium": (
        "event",
        """
        At Actium (31 BCE), [[augustus|Octavian's]] fleet under Agrippa
        defeated [[mark-antony]] and [[cleopatra]] off the coast of
        western Greece. Antony and Cleopatra fled to Egypt, both committed
        suicide within the year, and [[ptolemaic-egypt|Ptolemaic Egypt]]
        was annexed — completing Rome's absorption of the Hellenistic
        successor kingdoms.
        """,
        ["augustus", "mark-antony", "cleopatra", "ptolemaic-egypt"],
    ),
    "pax-romana": (
        "event",
        """
        The Pax Romana ("Roman peace") is the conventional name for the
        roughly two centuries (27 BCE – 180 CE) from [[augustus]] to
        [[marcus-aurelius]] during which the empire was relatively stable
        and free of major civil wars. It ended with the accession of
        [[commodus]] and the long crisis of the third century.
        """,
        ["augustus", "marcus-aurelius", "commodus"],
    ),
    "crisis-of-the-third-century": (
        "event",
        """
        The Crisis of the Third Century (235–284 CE) was a fifty-year
        spiral of civil war, plague, currency debasement, Sasanid
        Persian and Germanic invasion, and breakaway empires (the
        Gallic Empire in the west, the Palmyrene Empire under Zenobia
        in the east) in which the Roman Empire nearly disintegrated.
        Over twenty emperors and pretenders held power in fifty years,
        most ending violently. [[diocletian|Diocletian's]] Tetrarchy
        (284) imposed a new political settlement that ended the crisis
        at the cost of transforming the empire into a more militarized,
        more bureaucratic state — the "dominate" rather than the
        principate.
        """,
        ["diocletian"],
    ),
    "fall-of-the-republic": (
        "event",
        """
        The fall of the Roman Republic is conventionally dated to the
        period from the [[gallic-wars]] through the [[battle-of-actium]]
        — roughly 50–31 BCE. The institutions that had governed
        [[rome]] since the expulsion of the kings could not contain the
        wealth and armies generated by overseas conquest, and after two
        rounds of civil war (Caesar–Pompey, Octavian–Antony) the
        principate of [[augustus]] replaced them.
        """,
        ["gallic-wars", "battle-of-actium", "rome", "augustus"],
    ),
}

ROME_CONCEPTS: dict[str, tuple[str, str, list[str]]] = {
    "roman-republic": (
        "concept",
        """
        The Roman Republic (509–27 BCE) was the constitutional regime
        founded after the expulsion of Tarquinius Superbus and ended in
        practice by the [[fall-of-the-republic|civil wars]] that brought
        [[augustus]] to power. Its central institutions were the two
        annual consuls (with mutual veto), the Senate (advisory but in
        practice dominant), and the popular assemblies (comitia centuriata,
        tributa, and plebis). The "struggle of the orders" (494–287 BCE)
        gradually opened offices to plebeians and created the tribunate.
        It expanded from a city to a Mediterranean empire across these
        five centuries — and that expansion eventually destroyed it.
        """,
        ["fall-of-the-republic", "augustus"],
    ),
    "roman-empire": (
        "concept",
        """
        The Roman Empire is the post-republican Roman state (27 BCE –
        476 CE in the West, 1453 in the East as the
        [[byzantine-empire]]). Conventionally divided into the principate
        (from [[augustus]] to the [[crisis-of-the-third-century|crisis
        of the third century]]) and the dominate (from [[diocletian]]
        on, with overt monarchical court ritual). At its height under
        [[trajan]] it stretched from Britain to Mesopotamia and from
        the Rhine to the Sahara.
        """,
        ["byzantine-empire", "augustus", "crisis-of-the-third-century", "diocletian", "trajan"],
    ),
    "five-good-emperors": (
        "concept",
        """
        The "Five Good Emperors" — [[nerva]], [[trajan]], [[hadrian]],
        [[antoninus-pius]], and [[marcus-aurelius]] (96–180 CE) — is
        Machiavelli's coinage for the line of adoptive successors that
        gave the empire its longest stretch of competent rule. The
        succession by adoption rather than blood broke down when Marcus
        Aurelius passed the throne to his actual son [[commodus]] — the
        eighteenth-century historian Gibbon treated this moment as the
        beginning of Rome's long decline.
        """,
        ["nerva", "trajan", "hadrian", "antoninus-pius", "marcus-aurelius", "commodus"],
    ),
    "senate": (
        "concept",
        """
        The Roman Senate was an advisory council of c. 300 (later 600,
        under [[julius-caesar]] briefly 900) ex-magistrates who served
        for life. It had no formal legislative power but its *senatus
        consulta* carried such weight that for centuries no consul
        ignored them. Under the empire it retained ceremonial dignity
        while real power flowed to the emperor; [[tiberius]] preserved
        the forms, [[caligula]] mocked them, [[domitian]] terrorized it.
        """,
        ["julius-caesar", "tiberius", "caligula", "domitian"],
    ),
    "consul": (
        "concept",
        """
        Consul was the highest annual magistracy of the [[roman-republic]],
        held in pairs with mutual veto, with command of armies in the
        field. The office continued under the empire as honorific. Year
        names in Roman dating were the consuls' names ("in the
        consulship of so-and-so"). Famous consulships: [[cicero|Cicero's]]
        (63 BCE, Catiline), [[julius-caesar|Caesar's]] (59 BCE).
        """,
        ["roman-republic", "cicero", "julius-caesar"],
    ),
    "dictator": (
        "concept",
        """
        Dictator was an extraordinary Roman magistracy for a single
        emergency, capped at six months, with supreme authority and
        nominally above appeal. Used cleanly through the early Republic
        (Cincinnatus, called from his plough to defeat the Aequi and
        return to it in two weeks, is the proverbial example). [[sulla]]
        broke the form by holding it without time limit; [[julius-caesar]]
        broke it further by holding *dictator perpetuo*. After Caesar's
        murder the office was formally abolished.
        """,
        ["sulla", "julius-caesar"],
    ),
    "imperium": (
        "concept",
        """
        Imperium was the formal power of supreme command — military and
        civil — held by Roman magistrates: consuls, praetors,
        proconsuls, dictators. It was the basis of the emperor's
        position; [[augustus|Augustus']] settlement consolidated multiple
        kinds of imperium (proconsular over the militarized provinces,
        tribunician for the city) into a single dominant office without
        ever giving it a new name.
        """,
        ["augustus"],
    ),
    "pax-romana-concept": (
        "concept",
        """
        See the [[pax-romana]] event note for the period itself. The
        *concept* of a "Roman peace" — internal stability bought by
        unchallengeable military dominance — became a template invoked
        in every later imperial ideology, from the British and French
        empires to the American "Pax Americana."
        """,
        ["pax-romana"],
    ),
}

ROME_PLACES: dict[str, tuple[str, str, list[str]]] = {
    "rome": (
        "place",
        """
        Rome, traditionally founded by [[romulus-and-remus]] in 753 BCE on
        the Tiber, grew from city-state to Mediterranean empire over
        seven centuries. Republic (509 BCE – 27 BCE), then principate
        under [[augustus]], then dominate after [[diocletian]]. Sacked by
        Alaric in 410 and Geiseric in 455, with the western empire ending
        conventionally in 476. The eastern empire continued at
        [[constantinople]] for another millennium as the
        [[byzantine-empire]].
        """,
        ["romulus-and-remus", "augustus", "diocletian", "constantinople", "byzantine-empire"],
    ),
    "carthage": (
        "place",
        """
        Carthage, a Phoenician colony in modern Tunisia, was the great
        rival of [[rome]] in the western Mediterranean and the home of
        [[hannibal]]. Defeated in the three [[punic-wars]] and finally
        razed in 146 BCE under [[scipio-aemilianus]]. Later refounded by
        [[julius-caesar]] and [[augustus]] as a Roman colony.
        """,
        ["rome", "hannibal", "punic-wars", "scipio-aemilianus", "julius-caesar", "augustus"],
    ),
    "ptolemaic-egypt": (
        "place",
        """
        The Ptolemaic kingdom of Egypt (305–30 BCE), founded by
        [[ptolemy-i]] after the death of [[alexander-the-great]],
        capitaled at [[alexandria]]. Greek-speaking dynasty over a native
        Egyptian population; last ruler [[cleopatra]]; annexed by
        [[rome]] after the [[battle-of-actium]].
        """,
        ["ptolemy-i", "alexander-the-great", "alexandria", "cleopatra", "rome", "battle-of-actium"],
    ),
    "ptolemy-i": (
        "person",
        """
        Ptolemy I Soter (c. 367–282 BCE) was a general and bodyguard of
        [[alexander-the-great]] who founded the [[ptolemaic-egypt|Ptolemaic
        dynasty]] in Egypt after the wars of the Diadochi. He patronized
        the founding of the Library of [[alexandria]] and wrote a now-lost
        memoir of Alexander's campaigns that ancient historians relied on.
        """,
        ["alexander-the-great", "ptolemaic-egypt", "alexandria"],
    ),
}

# ─────────────────────────────────────────────────────────────────────────
# BYZANTIUM
# ─────────────────────────────────────────────────────────────────────────

BYZANTIUM: dict[str, tuple[str, str, list[str]]] = {
    "byzantine-empire": (
        "place",
        """
        The Byzantine Empire is the modern name for the medieval Roman
        Empire centered on [[constantinople]] (refounded from
        [[byzantium]] by [[constantine]] in 330 CE) and lasting until
        1453. Greek-speaking, Orthodox Christian, and continuous with
        the eastern Roman state, it preserved classical learning,
        codified Roman law under [[justinian]] (the [[corpus-juris-civilis]]),
        and shaped Eastern European and Russian civilization.
        """,
        ["constantinople", "byzantium", "constantine", "justinian", "corpus-juris-civilis"],
    ),
    "byzantium": (
        "place",
        """
        Byzantium was a Greek colony on the Bosphorus, refounded by
        [[constantine]] in 330 CE as Constantinople — *Nova Roma* — and
        capital of the [[byzantine-empire]] for over a thousand years.
        """,
        ["constantine", "byzantine-empire"],
    ),
    "constantinople": (
        "place",
        """
        Constantinople, the city [[constantine]] founded on the
        [[byzantium|Byzantium]] site in 330 CE, was the capital of the
        [[byzantine-empire]] until its fall to the Ottomans in 1453 under
        [[constantine-xi]]. Its Theodosian walls held off besiegers for
        eight centuries.
        """,
        ["constantine", "byzantium", "byzantine-empire", "constantine-xi"],
    ),
    "justinian": (
        "person",
        """
        Justinian I (c. 482–565), Byzantine emperor, sponsored the
        codification of Roman law as the [[corpus-juris-civilis]] under
        the jurist Tribonian, briefly reconquered Italy and North Africa
        through his general Belisarius, and built the Hagia Sophia in
        [[constantinople]]. His reign is the high noon of late antiquity.
        """,
        ["corpus-juris-civilis", "constantinople"],
    ),
    "theodora": (
        "person",
        """
        Theodora (c. 500–548), wife of [[justinian]] and Byzantine empress,
        rose from circus-actress background and reportedly stiffened her
        husband's nerve during the Nika riots ("the purple makes a fine
        shroud"). She was a major power at court and patroness of
        Monophysite churches.
        """,
        ["justinian"],
    ),
    "basil-ii": (
        "person",
        """
        Basil II (958–1025), called *Boulgaroktonos* — "Bulgar-Slayer" —
        was a Byzantine emperor who crushed the First Bulgarian Empire,
        stabilized the eastern frontier, and brought the
        [[byzantine-empire]] to its medieval peak.
        """,
        ["byzantine-empire"],
    ),
    "constantine-xi": (
        "person",
        """
        Constantine XI Palaiologos (1405–1453), last Byzantine emperor,
        died defending [[constantinople]] in its final siege by Mehmed
        II's Ottomans. His death conventionally marks the end of the
        Roman state, 1480 years after [[augustus]].
        """,
        ["constantinople", "augustus"],
    ),
    "corpus-juris-civilis": (
        "work",
        """
        The Corpus Juris Civilis ("body of civil law") was the codification
        of Roman law sponsored by [[justinian]] and completed in 534 CE.
        Rediscovered in 11th-century Italy, it became the foundation of
        the civil-law tradition that governs most of continental Europe
        and Latin America today.
        """,
        ["justinian"],
    ),
}

# ─────────────────────────────────────────────────────────────────────────
# MYTHOLOGY
# ─────────────────────────────────────────────────────────────────────────

MYTH: dict[str, tuple[str, str, list[str]]] = {
    "zeus": (
        "deity",
        """
        Zeus, king of the Olympian gods in Greek myth, son of Cronus and
        Rhea, husband and brother of [[hera]]. Sky-father and weather-god,
        wielder of the thunderbolt, dispenser of justice and oaths. His
        children include [[athena]], [[apollo]], [[artemis]], [[ares]],
        [[hermes]], [[dionysus]], [[heracles]], and many more. Romans
        identified him with [[jupiter]].
        """,
        ["hera", "athena", "apollo", "artemis", "ares", "hermes", "dionysus", "heracles", "jupiter"],
    ),
    "hera": (
        "deity",
        """
        Hera, queen of the Olympians, wife of [[zeus]], goddess of marriage
        and childbirth. In myth she relentlessly persecutes Zeus' illegitimate
        children, most famously [[heracles]]. Roman [[juno]].
        """,
        ["zeus", "heracles", "juno"],
    ),
    "athena": (
        "deity",
        """
        Athena, goddess of wisdom, war strategy, and craft, daughter of
        [[zeus]] born from his head fully armed. Patron of [[athens]],
        which won her favor over [[poseidon]] in the contest for the city.
        Roman [[minerva]].
        """,
        ["zeus", "athens", "poseidon", "minerva"],
    ),
    "apollo": (
        "deity",
        """
        Apollo, god of light, prophecy, music, healing, and the arts;
        twin of [[artemis]] and son of [[zeus]] and Leto. His oracle at
        Delphi was the most important in the Greek world.
        """,
        ["artemis", "zeus"],
    ),
    "artemis": (
        "deity",
        """
        Artemis, goddess of the hunt, wilderness, and the moon; virgin
        twin of [[apollo]]. Roman [[diana]].
        """,
        ["apollo", "diana"],
    ),
    "ares": (
        "deity",
        """
        Ares, god of war in its violent and chaotic aspect, son of [[zeus]]
        and [[hera]]. The Greeks viewed him with notable ambivalence; the
        Romans, who equated him with [[mars]], gave him much higher status.
        """,
        ["zeus", "hera", "mars"],
    ),
    "poseidon": (
        "deity",
        """
        Poseidon, god of the sea, earthquakes, and horses, brother of
        [[zeus]] and [[hades]]. He bears a long grudge against [[odysseus]]
        through the [[odyssey]]. Roman [[neptune]].
        """,
        ["zeus", "hades", "odysseus", "odyssey", "neptune"],
    ),
    "hades": (
        "deity",
        """
        Hades, lord of the underworld, brother of [[zeus]] and [[poseidon]].
        Husband of Persephone, daughter of Demeter, whose annual
        return from the underworld explains the seasons.
        """,
        ["zeus", "poseidon"],
    ),
    "dionysus": (
        "deity",
        """
        Dionysus, god of wine, theater, ecstasy, and ritual madness, son of
        [[zeus]] and a mortal Semele. His worship — Dionysiac frenzy —
        appears in [[euripides|Euripides']] Bacchae. Roman Bacchus.
        """,
        ["zeus", "euripides"],
    ),
    "hermes": (
        "deity",
        """
        Hermes, messenger of the gods, conductor of souls to the
        underworld, patron of travelers, traders, and thieves. Son of
        [[zeus]] and Maia. Roman [[mercury]].
        """,
        ["zeus", "mercury"],
    ),
    "aphrodite": (
        "deity",
        """
        Aphrodite, goddess of love and beauty, born from the sea-foam at
        Cyprus after the castration of Uranus (or, in [[homer]], daughter
        of [[zeus]] and Dione). Mother of [[aeneas]] by Anchises. Roman
        [[venus]].
        """,
        ["homer", "zeus", "aeneas", "venus"],
    ),
    "achilles": (
        "person",
        """
        Achilles, greatest of the Greek heroes at Troy, son of the
        sea-nymph Thetis and the mortal Peleus, central figure of the
        [[iliad]]. His wrath at Agamemnon and his killing of [[hector]] are
        the poem's twin axes. Slain by Paris with an arrow guided by
        [[apollo]].
        """,
        ["iliad", "hector", "apollo"],
    ),
    "hector": (
        "person",
        """
        Hector, eldest son of Priam and Hecuba, greatest Trojan warrior,
        defender of Troy, slain by [[achilles]] in the [[iliad]]. His
        funeral closes the poem.
        """,
        ["achilles", "iliad"],
    ),
    "odysseus": (
        "person",
        """
        Odysseus, king of Ithaca, hero of [[homer|Homer's]] [[odyssey]] and
        a major figure in the [[iliad]]. Cunning, eloquent, and often
        ruthless. His ten-year voyage home from the [[trojan-war]] is the
        archetype of the journey-romance.
        """,
        ["homer", "odyssey", "iliad", "trojan-war"],
    ),
    "aeneas": (
        "person",
        """
        Aeneas, Trojan prince, son of [[aphrodite]] and Anchises, hero of
        [[virgil|Virgil's]] [[aeneid]]. Escapes the fall of Troy carrying
        his father on his back and leads survivors to Italy, where his
        descendants will found [[rome]].
        """,
        ["aphrodite", "virgil", "aeneid", "rome"],
    ),
    "heracles": (
        "person",
        """
        Heracles (Roman Hercules), greatest of Greek heroes, son of
        [[zeus]] and the mortal Alcmene, hounded by [[hera]] through the
        Twelve Labors. Strangled snakes in his cradle; held up the sky in
        place of Atlas; died from the poisoned shirt of Nessus.
        """,
        ["zeus", "hera"],
    ),
    "theseus": (
        "person",
        """
        Theseus, mythical hero-king of [[athens]], slayer of the Minotaur
        in the Cretan labyrinth, unifier of Attica. The Athenians treated
        him as their founder-hero — their answer to the
        [[romulus-and-remus|Roman]] Romulus.
        """,
        ["athens", "romulus-and-remus"],
    ),
    "perseus": (
        "person",
        """
        Perseus, son of [[zeus]] and Danaë, slayer of the Gorgon Medusa,
        rescuer of Andromeda. Mythical founder of Mycenae.
        """,
        ["zeus"],
    ),
    "jason": (
        "person",
        """
        Jason, leader of the Argonauts, retrieved the Golden Fleece from
        Colchis with the help of the sorceress Medea — whose later revenge
        is the subject of [[euripides|Euripides']] tragedy.
        """,
        ["euripides"],
    ),
    "jupiter": (
        "deity",
        """
        Jupiter, chief god of the Roman pantheon, equated with Greek
        [[zeus]]. Worshipped on the Capitoline as Jupiter Optimus Maximus.
        """,
        ["zeus"],
    ),
    "juno": (
        "deity",
        """
        Juno, Roman queen of the gods, wife of [[jupiter]], equated with
        Greek [[hera]]. Patroness of marriage and the matronae.
        """,
        ["jupiter", "hera"],
    ),
    "mars": (
        "deity",
        """
        Mars, Roman god of war (less ambivalently honored than Greek
        [[ares]]), reputed father of [[romulus-and-remus]] and so a
        founder-deity of [[rome]] itself.
        """,
        ["ares", "romulus-and-remus", "rome"],
    ),
    "minerva": (
        "deity",
        """
        Minerva, Roman goddess of wisdom, war strategy, and craft, equated
        with Greek [[athena]]. One of the Capitoline Triad with [[jupiter]]
        and [[juno]].
        """,
        ["athena", "jupiter", "juno"],
    ),
    "venus": (
        "deity",
        """
        Venus, Roman goddess of love and beauty, equated with
        [[aphrodite]]. Claimed as ancestress of the Julian gens through
        [[aeneas]] — and hence of [[julius-caesar]] and [[augustus]].
        """,
        ["aphrodite", "aeneas", "julius-caesar", "augustus"],
    ),
    "mercury": (
        "deity",
        """
        Mercury, Roman god of commerce, communication, and travelers,
        equated with [[hermes]].
        """,
        ["hermes"],
    ),
    "neptune": (
        "deity",
        """
        Neptune, Roman god of the sea, equated with [[poseidon]].
        """,
        ["poseidon"],
    ),
    "diana": (
        "deity",
        """
        Diana, Roman goddess of the hunt, wild places, and the moon,
        equated with [[artemis]].
        """,
        ["artemis"],
    ),
}

# ─────────────────────────────────────────────────────────────────────────
# Build it all
# ─────────────────────────────────────────────────────────────────────────

SECTIONS = [
    ("greece/people", GREECE_PEOPLE),
    ("greece/literature", GREECE_WRITERS),
    ("greece/works", GREECE_WORKS),
    ("greece/events", GREECE_EVENTS),
    ("greece/places", GREECE_PLACES),
    ("greece/concepts", GREECE_CONCEPTS),
    ("rome/people", ROME_PEOPLE),
    ("rome/literature", ROME_WRITERS),
    ("rome/works", ROME_WORKS),
    ("rome/events", ROME_EVENTS),
    ("rome/places", ROME_PLACES),
    ("rome/concepts", ROME_CONCEPTS),
    ("byzantium", BYZANTIUM),
    ("mythology", MYTH),
]


def main() -> None:
    if KB.exists():
        shutil.rmtree(KB)
    KB.mkdir(parents=True)

    title_map: dict[str, str] = {}  # slug -> human title
    folder_for: dict[str, str] = {}  # slug -> folder

    # Pass 1: collect all titles (for nice rendering of orphan links later)
    for folder, mapping in SECTIONS:
        for slug_, (kind, _body, _links) in mapping.items():
            title_map[slug_] = slug_.replace("-", " ").title()
            folder_for[slug_] = folder

    counts: dict[str, int] = {}
    for folder, mapping in SECTIONS:
        out_dir = KB / folder
        out_dir.mkdir(parents=True, exist_ok=True)
        for slug_, (kind, body, links) in mapping.items():
            md = note(
                title=title_map[slug_],
                kind=kind,
                body=body,
                links=links,
            )
            (out_dir / f"{slug_}.md").write_text(md, encoding="utf-8")
        counts[folder] = len(mapping)

    # Top-level index
    lines = ["---", "tags: [type/index]", "created: 2026-05-11", "---", "", "# Classical Knowledge Base", "",
             "A benchmark corpus on Greco-Roman antiquity for retrieval comparison",
             "(BM25 / dense vectors / spiking cortex). Hand-authored stubs, link-dense.",
             "", "## Sections", ""]
    for folder, mapping in SECTIONS:
        lines.append(f"- **{folder}** — {len(mapping)} notes")
    lines += ["", "## Stats", "",
              f"- Total notes: **{sum(counts.values())}**",
              f"- Total link references: **{sum(len(t[2]) for _, m in SECTIONS for t in m.values())}**",
              ""]
    (KB / "index.md").write_text("\n".join(lines), encoding="utf-8")

    print(f"wrote {sum(counts.values())} notes across {len(SECTIONS)} folders → {KB}")


if __name__ == "__main__":
    main()
