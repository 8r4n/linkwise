const fields = [
  "examples", "performance", "caller", "listener", "exchange", "received",
  "payload", "payloadNote", "turn", "turnNote", "limits", "bench", "notes",
  "readyPrompt", "readyReply", "confirmed",
];

const translations = {
  en: [
    "Workflow examples", "Performance against VARA HF", "Caller", "Listener",
    "Modem exchange", "Open the received message", "Exact payload delivery",
    "In the recorded BW500 audio bench, each route delivered the same 203-byte send, 6-byte reply, and 203-byte continuation exactly.",
    "Time to resume DATA", "One historical BW500 trace per route, measured from the caller’s acknowledgment unkeying to its next DATA transmission. Linkwise was the responder in the mixed trace.",
    "These results establish parity for exact delivery of this fixture. Matched throughput, weak-signal reliability, and on-air performance remain unverified.",
    "Bench procedure and results", "Lab notes", "Ready for the check-in?", "Ready.", "Confirmed.",
  ],
  "zh-Hans": [
    "操作示例", "与 VARA HF 的性能对比", "主叫方", "监听方",
    "调制解调器交换", "打开收到的邮件", "有效载荷完整交付",
    "在记录的 BW500 音频台架测试中，每条链路均完整传输了 203 字节的初始数据、6 字节的回复和 203 字节的后续数据。",
    "恢复 DATA 发送所需时间", "每条链路使用一条历史 BW500 记录，从主叫方确认信号结束至下一次 DATA 发送计时。混合链路由 Linkwise 应答。",
    "在此测试样例中，各链路的交付完整性相同。相同条件下的吞吐量、弱信号可靠性和实际无线电性能仍待验证。",
    "测试流程与结果", "实验记录", "准备好联络了吗？", "准备好了。", "已确认。",
  ],
  hi: [
    "कार्यप्रवाह के उदाहरण", "VARA HF से प्रदर्शन की तुलना", "कॉल करने वाला", "सुनने वाला",
    "मॉडेम से आदान-प्रदान", "प्राप्त संदेश खोलें", "डेटा की हूबहू डिलीवरी",
    "रिकॉर्ड किए गए BW500 ऑडियो परीक्षण में हर मार्ग ने 203 बाइट का प्रेषण, 6 बाइट का जवाब और अगले 203 बाइट हूबहू पहुँचाए।",
    "DATA दोबारा भेजने तक का समय", "हर मार्ग के लिए एक पुराना BW500 रिकॉर्ड लिया गया। समय कॉलर की पावती का प्रसारण बंद होने से अगला DATA प्रसारण शुरू होने तक का है। मिश्रित मार्ग में Linkwise उत्तरदाता था।",
    "इस परीक्षण में सभी मार्गों ने डेटा हूबहू पहुँचाया। समान परिस्थितियों में थ्रूपुट, कमज़ोर सिग्नल पर विश्वसनीयता और रेडियो प्रसारण का प्रदर्शन अभी सत्यापित होना बाकी है।",
    "परीक्षण की विधि और परिणाम", "प्रयोगशाला के नोट्स", "संपर्क के लिए तैयार हैं?", "तैयार हूँ।", "पुष्टि हुई।",
  ],
  es: [
    "Ejemplos de uso", "Rendimiento frente a VARA HF", "Estación llamante", "Estación a la escucha",
    "Intercambio por módem", "Abre el mensaje recibido", "Entrega exacta de datos",
    "En el ensayo de audio BW500 registrado, cada ruta entregó exactamente el mismo envío de 203 bytes, la respuesta de 6 bytes y la continuación de 203 bytes.",
    "Tiempo hasta reanudar DATA", "Una traza histórica BW500 por ruta, desde el final de la transmisión del acuse de la estación llamante hasta su siguiente envío DATA. Linkwise respondió en la ruta mixta.",
    "Las rutas lograron la misma entrega exacta en esta prueba. Queda por verificar el caudal en condiciones equivalentes, la fiabilidad con señales débiles y el rendimiento por radio.",
    "Procedimiento y resultados del ensayo", "Notas de laboratorio", "¿Todo listo para el contacto?", "Todo listo.", "Confirmado.",
  ],
  ar: [
    "أمثلة على سير العمل", "مقارنة الأداء مع VARA HF", "المحطة المتصلة", "المحطة المستمعة",
    "التبادل عبر المودم", "افتح الرسالة المستلمة", "تسليم البيانات دون تغيير",
    "في اختبار الصوت المسجّل بنمط BW500، سلّم كل مسار البيانات نفسها بدقة: إرسالاً من 203 بايتات، ورداً من 6 بايتات، وتتمةً من 203 بايتات.",
    "زمن استئناف إرسال DATA", "استُخدم تسجيل تاريخي واحد لكل مسار في نمط BW500. يبدأ القياس عند انتهاء بث التأكيد من المحطة المتصلة وينتهي عند إرسال DATA التالي. تولّى Linkwise دور المحطة المجيبة في المسار المختلط.",
    "تكافأت المسارات في تسليم بيانات هذه التجربة دون تغيير. ولا يزال معدل النقل في ظروف متكافئة، والموثوقية مع الإشارات الضعيفة، والأداء عبر الأثير بحاجة إلى التحقق.",
    "إجراءات الاختبار ونتائجه", "ملاحظات المختبر", "هل أنت جاهز للاتصال؟", "أنا جاهز.", "تم التأكيد.",
  ],
  fr: [
    "Exemples d’utilisation", "Performances comparées à VARA HF", "Station appelante", "Station à l’écoute",
    "Échange par modem", "Ouvrir le message reçu", "Livraison à l’octet près",
    "Dans les essais audio BW500 enregistrés, chaque liaison a transmis à l’identique un envoi de 203 octets, une réponse de 6 octets et une suite de 203 octets.",
    "Délai avant la reprise de DATA", "Une trace BW500 historique par liaison, de la fin d’émission de l’accusé de réception de la station appelante à sa prochaine émission DATA. Linkwise répondait sur la liaison mixte.",
    "Les liaisons ont livré les mêmes données à l’identique dans cet essai. Le débit à conditions équivalentes, la fiabilité sous signal faible et les performances sur les ondes restent à vérifier.",
    "Protocole et résultats des essais", "Notes de laboratoire", "Tu es prêt pour le contact ?", "Prêt.", "Confirmé.",
  ],
  bn: [
    "কাজের উদাহরণ", "VARA HF-এর সঙ্গে কর্মক্ষমতার তুলনা", "কলকারী স্টেশন", "শ্রবণরত স্টেশন",
    "মডেমে আদান-প্রদান", "প্রাপ্ত বার্তা খুলুন", "হুবহু ডেটা সরবরাহ",
    "রেকর্ড করা BW500 অডিও পরীক্ষায় প্রতিটি পথে একই ২০৩ বাইটের প্রেরণ, ৬ বাইটের উত্তর এবং পরের ২০৩ বাইট হুবহু পৌঁছেছে।",
    "DATA আবার পাঠাতে সময়", "প্রতিটি পথের জন্য একটি পুরোনো BW500 রেকর্ড নেওয়া হয়েছে। কলকারীর স্বীকৃতি সম্প্রচার শেষ হওয়া থেকে পরের DATA সম্প্রচার পর্যন্ত সময় মাপা হয়েছে। মিশ্র পথে Linkwise উত্তরদাতা ছিল।",
    "এই পরীক্ষায় সব পথ হুবহু একই ডেটা সরবরাহ করেছে। সমতুল্য পরিবেশে ডেটার গতি, দুর্বল সংকেতে নির্ভরযোগ্যতা ও বেতার কর্মক্ষমতা এখনও যাচাই করা হয়নি।",
    "পরীক্ষার পদ্ধতি ও ফলাফল", "ল্যাব নোট", "যোগাযোগের জন্য প্রস্তুত?", "প্রস্তুত।", "নিশ্চিত হলো।",
  ],
  pt: [
    "Exemplos de utilização", "Desempenho comparado ao VARA HF", "Estação chamadora", "Estação à escuta",
    "Troca pelo modem", "Abra a mensagem recebida", "Entrega exata dos dados",
    "No ensaio de áudio BW500 registrado, cada rota entregou exatamente o mesmo envio de 203 bytes, a resposta de 6 bytes e a continuação de 203 bytes.",
    "Tempo até retomar DATA", "Um registro histórico BW500 por rota, medido do fim da transmissão da confirmação pela estação chamadora até sua próxima transmissão DATA. Linkwise respondeu na rota mista.",
    "As rotas entregaram os mesmos dados com exatidão nesse ensaio. A taxa de transferência em condições equivalentes, a confiabilidade com sinais fracos e o desempenho no ar ainda precisam ser verificados.",
    "Procedimento e resultados do ensaio", "Notas de laboratório", "Tudo pronto para o contato?", "Tudo pronto.", "Confirmado.",
  ],
  id: [
    "Contoh alur kerja", "Perbandingan kinerja dengan VARA HF", "Stasiun pemanggil", "Stasiun pendengar",
    "Pertukaran melalui modem", "Buka pesan yang diterima", "Pengiriman data utuh",
    "Pada uji audio BW500 yang direkam, setiap jalur mengirim rangkaian yang sama secara utuh, yaitu 203 byte kiriman, 6 byte balasan, dan 203 byte lanjutan.",
    "Waktu hingga pengiriman DATA berlanjut", "Satu rekaman BW500 historis per jalur. Waktu dihitung sejak pemanggil berhenti memancarkan konfirmasi hingga mulai mengirim DATA berikutnya. Linkwise menjadi penjawab pada jalur campuran.",
    "Semua jalur mengirim data secara utuh dalam uji ini. Laju transfer pada kondisi setara, keandalan pada sinyal lemah, dan kinerja melalui radio masih perlu diverifikasi.",
    "Prosedur dan hasil uji", "Catatan laboratorium", "Siap untuk kontak?", "Siap.", "Dikonfirmasi.",
  ],
  ru: [
    "Примеры работы", "Сравнение с VARA HF", "Вызывающая станция", "Ожидающая станция",
    "Обмен через модем", "Откройте полученное письмо", "Точная доставка данных",
    "В записанном аудиоиспытании BW500 по каждому маршруту без изменений прошли 203 байта, ответ из 6 байт и продолжение из 203 байт.",
    "Время до возобновления передачи DATA", "По одной исторической записи BW500 на маршрут. Измерен промежуток от окончания передачи подтверждения вызывающей станцией до её следующей передачи DATA. В смешанном соединении отвечал Linkwise.",
    "Точность доставки в этом испытании одинакова. Пропускная способность при одинаковых условиях, надёжность при слабом сигнале и характеристики в эфире ещё не проверены.",
    "Методика и результаты испытаний", "Лабораторный журнал", "Готов к сеансу связи?", "Готов.", "Подтверждаю.",
  ],
  de: [
    "Anwendungsbeispiele", "Leistung im Vergleich zu VARA HF", "Rufende Station", "Empfangsbereite Station",
    "Austausch über das Modem", "Empfangene Nachricht öffnen", "Bytegenaue Zustellung",
    "Im aufgezeichneten BW500-Audiotest übertrug jede Verbindung dieselben 203 Byte, die 6 Byte lange Antwort und die folgenden 203 Byte fehlerfrei.",
    "Zeit bis zur nächsten DATA-Sendung", "Eine historische BW500-Aufzeichnung je Verbindung, gemessen vom Ende der gesendeten Bestätigung der rufenden Station bis zu ihrer nächsten DATA-Sendung. In der gemischten Verbindung antwortete Linkwise.",
    "In diesem Test war die Zustellgenauigkeit gleich. Datendurchsatz unter gleichen Bedingungen, Zuverlässigkeit bei schwachen Signalen und Leistung im Funkbetrieb bleiben zu prüfen.",
    "Testverfahren und Ergebnisse", "Labornotizen", "Bereit für die Funkrunde?", "Bereit.", "Bestätigt.",
  ],
};

export function visualGuide(code) {
  const labels = translations[code];
  if (labels?.length !== fields.length) throw new Error("Invalid visual guide translation for " + code);
  return Object.fromEntries(fields.map((field, index) => [field, labels[index]]));
}
