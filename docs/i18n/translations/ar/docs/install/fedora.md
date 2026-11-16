# نسخة مودم Linkwise الأولية لنظام Fedora

يوفر Linkwise مودمًا برمجيًا ثنائي الاتجاه وتشخيصًا للمحطة ومحاكاة محلية. راجع Station Setup لتوجيه الصوت وإعدادات الإرسال.

لا يتضمن مثبّت النسخة الأولية ميزة `secure-transfer`. استخدم بناءً من المصدر لنقل الملفات باستخدام gzip/SHA-256، وبنية PKI المشتركة، والمحادثة والبريد المحميين، وبطاقات QSL للشهادات. يوضح [دليل PKI](../../../../../../rust/docs/pki.md) الإعداد ومتطلبات المحادثة الهجينة.

## التثبيت أو التحديث

نفّذ `uname -m` لمعرفة معمارية الجهاز. نفّذ الأوامر التالية في مجلد التنزيل واستبدل اسم المثال باسم الحزمة المطابقة.

```sh
sudo dnf install ./linkwise-modem-0.1.0-1.aarch64.rpm
linkwise-modemd --version
linkwise-modemd --simulate
```

يثبّت DNF مكتبات النظام المطلوبة. للتحديث استخدم `sudo dnf upgrade ./FILE.rpm` مع الملف الأحدث. تعمل المحاكاة محليًا من دون راديو.

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

### SELinux

أبق SELinux في وضع enforcing. تثبّت حزمة RPM سياسة خدمة النظام. بعد إنشاء الإعداد أو استبداله أعد الوسوم وتحقّق من النطاق العامل.

```sh
sudo restorecon -Rv /etc/linkwise
ls -Z /usr/bin/linkwise-modemd /etc/linkwise/station.json
ps -Z -p "$(systemctl show -p MainPID --value linkwise-modem)"
sudo ausearch -m AVC,USER_AVC -ts recent -i
```

الوسوم المتوقعة هي `linkwise_modem_exec_t` للبرنامج، و`linkwise_modem_etc_t` للإعداد، و`linkwise_modem_t` لخدمة النظام. يحمل منفذا TCP الافتراضيان 8300 و8301 النوع `linkwise_modem_port_t`. عند استخدام منافذ أخرى عيّن النوع نفسه وحل أي تعارض وسوم. تعمل خدمة سطح المكتب في نطاق المستخدم الذي شغّلها.

اختبار تطبيق SELinux في Fedora وتأهيل DISA STIG فحصان مستقلان. أهّل صورة النظام المشمولة بـSTIG ومتغير السياسة وإعداد الموقع المرجعي قبل النشر.

## إزالة التثبيت

أوقف أوامر Linkwise الجارية ثم نفّذ `sudo dnf remove linkwise-modem`. تبقى ملفات المحطة في دليلك الشخصي متاحة.
