# نسخة مودم Linkwise الأولية لنظام macOS

يوفر Linkwise مودمًا برمجيًا ثنائي الاتجاه وتشخيصًا للمحطة ومحاكاة محلية. راجع Station Setup لتوجيه الصوت وإعدادات الإرسال.

لا يتضمن مثبّت النسخة الأولية ميزة `secure-transfer`. استخدم بناءً من المصدر لنقل الملفات باستخدام gzip/SHA-256، وبنية PKI المشتركة، والمحادثة والبريد المحميين، وبطاقات QSL للشهادات. يوضح [دليل PKI](../../../../../../rust/docs/pki.md) الإعداد ومتطلبات المحادثة الهجينة.

## التثبيت أو التحديث

1. تحقق من نوع الشريحة أو المعالج ضمن قائمة Apple → About This Mac. استخدم `macos-arm64.pkg` لأجهزة Apple silicon أو `macos-x86_64.pkg` لأجهزة Intel.
2. افتح الحزمة في تطبيق Installer واتبع التعليمات. يتطلب التثبيت حساب مسؤول. للتحديث شغّل الحزمة الأحدث بالطريقة نفسها. احفظ إعداد المحطة في دليلك الشخصي.
3. افتح Terminal ونفّذ الأوامر التالية.

   ```sh
   /usr/local/bin/linkwise-modemd --version
   /usr/local/bin/linkwise-modemd --simulate
   ```

تعمل المحاكاة محليًا من دون راديو. قد تكون حزم التطوير غير موقّعة. إذا رفض macOS الحزمة، اطلب نسخة موقّعة وموثّقة من الموزع.

## إعداد المحطة

نفّذ `man 8 linkwise-modemd` لمرجع الأوامر و`man 5 linkwise-station` لصيغة الإعداد. افتح `/usr/local/share/doc/linkwise-modem/Station-setup.md` لاختيار الأجهزة وإنشاء `station.json`. إذا رُفض الالتقاط، تحقّق من إذن Terminal في System Settings → Privacy & Security → Microphone.

## تشغيل وكيل تسجيل دخول المودم

في macOS 13 أو أحدث، احفظ الإعداد في `~/Library/Application Support/Linkwise/station.json` ثم نفّذ الأمر التالي.

```sh
"/Applications/Linkwise Station.app/Contents/MacOS/LinkwiseStation" enable
```

إذا طُلب منك ذلك، اسمح باستخدام الميكروفون ووافق على عنصر الخلفية في System Settings. استخدم الأمر نفسه مع `status` لمعرفة حالة التسجيل أو مع `disable` لإلغاء تسجيل الوكيل قبل إزالة التثبيت.

## إزالة التثبيت

أوقف أي أوامر Linkwise قيد التشغيل ثم أزل الملفات المثبّتة وسجل تثبيت الحزمة.

```sh
sudo rm -f /usr/local/bin/linkwise /usr/local/bin/linkwise-modemd \
  /usr/local/share/doc/linkwise-modem/Install.md \
  /usr/local/share/doc/linkwise-modem/LICENSE \
  /usr/local/share/doc/linkwise-modem/THIRD-PARTY-NOTICES.txt \
  /usr/local/share/doc/linkwise-modem/Station-setup.md \
  /usr/local/share/man/man1/linkwise.1 \
  /usr/local/share/man/man8/linkwise-modemd.8 \
  /usr/local/share/man/man5/linkwise-station.5
sudo rm -rf "/Applications/Linkwise Station.app"
sudo pkgutil --forget io.github.8r4n.linkwise.modem
```

تبقى ملفات المحطة المحفوظة في دليلك الشخصي متاحة.
