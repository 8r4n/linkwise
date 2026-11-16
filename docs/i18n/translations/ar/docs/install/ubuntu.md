# نسخة مودم Linkwise الأولية لنظام Ubuntu

يوفر Linkwise مودمًا برمجيًا ثنائي الاتجاه وتشخيصًا للمحطة ومحاكاة محلية. راجع Station Setup لتوجيه الصوت وإعدادات الإرسال.

لا يتضمن مثبّت النسخة الأولية ميزة `secure-transfer`. استخدم بناءً من المصدر لنقل الملفات باستخدام gzip/SHA-256، وبنية PKI المشتركة، والمحادثة والبريد المحميين، وبطاقات QSL للشهادات. يوضح [دليل PKI](../../../../../../rust/docs/pki.md) الإعداد ومتطلبات المحادثة الهجينة.

## التثبيت أو التحديث

نفّذ `dpkg --print-architecture` لمعرفة معمارية الجهاز. نفّذ الأوامر التالية في مجلد التنزيل واستبدل اسم المثال باسم الحزمة المطابقة.

```sh
sudo apt install ./linkwise-modem_0.1.0_arm64.deb
linkwise-modemd --version
linkwise-modemd --simulate
```

يثبّت APT مكتبات النظام المطلوبة. للتحديث استخدم أمر التثبيت نفسه مع الملف الأحدث. تعمل المحاكاة محليًا من دون راديو.

## إعداد المحطة

راجع `man 8 linkwise-modemd` للأوامر و`man 5 linkwise-station` لصيغة الإعداد. افتح `/usr/share/doc/linkwise-modem/Station-setup.md` لاختيار الأجهزة وإنشاء `station.json`. نفّذ فحص الصوت بصفة مستخدم سطح المكتب للوصول إلى جلسة الصوت.

## تشغيل خدمة المودم

لمحطة سطح المكتب، احفظ الإعداد في `~/.config/linkwise/station.json` ثم نفّذ الأوامر التالية.

```sh
systemctl --user enable --now linkwise-modem.service
journalctl --user -u linkwise-modem.service
```

للمحطة المخصصة ضع ملفات الإعداد والقياس المملوكة للمسؤول في `/etc/linkwise`. تحتاج الخدمة إلى قراءتها والوصول مباشرة إلى مدخل ومخرج ALSA المحددين. استخدم خدمة واحدة لكل مجموعة أجهزة ومنافذ.

```sh
sudo systemctl enable --now linkwise-modem.service
sudo journalctl -u linkwise-modem.service
```

عند تحديد `bandwidth` يفتح المودم منفذي الأوامر والبيانات على استرجاع IPv4. في غياب الحقل يقدم منفذ الأوامر تشخيص الاستقبال بصيغة JSON. اقرأه باستخدام `nc 127.0.0.1 8300`.

## إزالة التثبيت

أوقف أوامر Linkwise الجارية ثم نفّذ `sudo apt remove linkwise-modem`. تبقى ملفات المحطة في دليلك الشخصي متاحة.
