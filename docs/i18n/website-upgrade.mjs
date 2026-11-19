const fields = [
  "capabilities", "demos", "research", "watch", "build", "releaseNotes", "reportBug", "knownIssues",
  "overview", "available", "pending",
];

const labels = {
  en: ["Capabilities", "Demos", "Research", "Watch a demo", "Build from source", "Release notes", "Report a bug", "Known issues",
    "An open-source Rust HF modem for amateur radio. Explore BW500 chat, Winlink mail, and the tests behind each capability.",
    "Public installers are available", "Public preview installers are pending. You can run the demos from a source build."],
  "zh-Hans": ["功能", "演示", "研究", "观看演示", "从源代码构建", "发布说明", "报告缺陷", "已知问题",
    "面向业余无线电的开源 Rust 短波调制解调器。了解 BW500 聊天、Winlink 邮件及各项功能的测试。",
    "公开安装包已发布", "公开预览版安装包尚未发布。您可以从源代码构建并运行演示。"],
  hi: ["क्षमताएँ", "प्रदर्शन", "शोध", "प्रदर्शन देखें", "स्रोत से बनाएँ", "रिलीज़ नोट्स", "त्रुटि की रिपोर्ट करें", "ज्ञात समस्याएँ",
    "शौकिया रेडियो के लिए एक ओपन सोर्स Rust HF मॉडेम। BW500 चैट, Winlink मेल और हर क्षमता के परीक्षण देखें।",
    "सार्वजनिक इंस्टॉलर उपलब्ध हैं", "सार्वजनिक पूर्वावलोकन के इंस्टॉलर अभी उपलब्ध नहीं हैं। स्रोत से बनाकर प्रदर्शन चला सकते हैं।"],
  es: ["Funciones", "Demostraciones", "Investigación", "Ver una demostración", "Compilar desde el código fuente", "Notas de la versión", "Informar de un fallo", "Problemas conocidos",
    "Módem HF de código abierto en Rust para radioafición. Explora el chat BW500, el correo Winlink y las pruebas de cada función.",
    "Instaladores públicos disponibles", "Los instaladores de la vista previa aún no están disponibles. Puedes ejecutar las demostraciones compilando el código fuente."],
  ar: ["القدرات", "العروض", "الأبحاث", "شاهد عرضًا", "البناء من المصدر", "ملاحظات الإصدار", "أبلغ عن خلل", "المشكلات المعروفة",
    "مودم HF مفتوح المصدر بلغة Rust لهواة الراديو. تعرّف على دردشة BW500 وبريد Winlink والاختبارات التي تدعم كل وظيفة.",
    "المثبّتات متاحة للعموم", "مثبّتات الإصدار التجريبي العام قيد الانتظار. يمكنك بناء البرنامج من المصدر لتشغيل العروض."],
  fr: ["Fonctionnalités", "Démonstrations", "Recherche", "Voir une démonstration", "Compiler depuis les sources", "Notes de version", "Signaler un bug", "Problèmes connus",
    "Modem HF libre en Rust pour les radioamateurs. Découvrez le chat BW500, le courrier Winlink et les essais de chaque fonction.",
    "Installateurs publics disponibles", "Les installateurs de la préversion publique sont en attente. Vous pouvez compiler les sources pour lancer les démonstrations."],
  bn: ["সক্ষমতা", "প্রদর্শনী", "গবেষণা", "প্রদর্শনী দেখুন", "সোর্স থেকে বিল্ড করুন", "রিলিজ নোট", "ত্রুটি জানান", "জানা সমস্যা",
    "অ্যামেচার রেডিওর জন্য Rust-এ লেখা উন্মুক্ত সোর্সের HF মডেম। BW500 চ্যাট, Winlink মেইল ও প্রতিটি সক্ষমতার পরীক্ষা দেখুন।",
    "সার্বজনীন ইনস্টলার উপলব্ধ", "সার্বজনীন প্রিভিউ ইনস্টলার এখনো প্রকাশিত হয়নি। সোর্স থেকে বিল্ড করে প্রদর্শনী চালানো যাবে।"],
  pt: ["Funcionalidades", "Demonstrações", "Pesquisa", "Ver uma demonstração", "Compilar a partir do código-fonte", "Notas da versão", "Relatar um erro", "Problemas conhecidos",
    "Modem HF de código aberto em Rust para radioamadores. Explore o chat BW500, o correio Winlink e os testes de cada funcionalidade.",
    "Instaladores públicos disponíveis", "Os instaladores da prévia pública ainda não estão disponíveis. Compile o código-fonte para executar as demonstrações."],
  id: ["Kemampuan", "Demo", "Riset", "Tonton demo", "Kompilasi dari kode sumber", "Catatan rilis", "Laporkan bug", "Masalah yang diketahui",
    "Modem HF sumber terbuka berbasis Rust untuk radio amatir. Lihat obrolan BW500, surel Winlink, dan pengujian setiap kemampuan.",
    "Penginstal publik tersedia", "Penginstal pratinjau publik belum tersedia. Kompilasi dari kode sumber untuk menjalankan demo."],
  ru: ["Возможности", "Демонстрации", "Исследования", "Смотреть демонстрацию", "Собрать из исходного кода", "Примечания к выпуску", "Сообщить об ошибке", "Известные проблемы",
    "КВ-модем с открытым исходным кодом на Rust для радиолюбителей. Посмотрите, как работают чат BW500 и почта Winlink и как их испытали.",
    "Установщики доступны для скачивания", "Установщики предварительной версии ещё не опубликованы. Демонстрации можно запустить после сборки из исходного кода."],
  de: ["Funktionen", "Demos", "Forschung", "Demo ansehen", "Aus dem Quellcode bauen", "Versionshinweise", "Fehler melden", "Bekannte Probleme",
    "Quelloffenes HF-Modem in Rust für den Amateurfunk. Entdecken Sie BW500-Chat, Winlink-Mail und die Tests zu jeder Funktion.",
    "Öffentliche Installationspakete verfügbar", "Installationspakete für die öffentliche Vorschau stehen noch aus. Die Demos lassen sich aus dem Quellcode bauen und ausführen."],
};

export function websiteLabels(code) {
  const values = labels[code];
  if (values?.length !== fields.length || values.some(value => !value.trim())) {
    throw new Error(`Incomplete website labels for ${code}`);
  }
  return Object.fromEntries(fields.map((field, index) => [field, values[index]]));
}
