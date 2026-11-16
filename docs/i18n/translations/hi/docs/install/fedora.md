# Fedora के लिए Linkwise मॉडेम पूर्वावलोकन

Linkwise दोतरफ़ा सॉफ़्टवेयर मॉडेम, स्टेशन निदान और स्थानीय सिमुलेशन देता है। ऑडियो रूटिंग और प्रसारण सेटिंग के लिए Station Setup देखें।

इस पूर्वावलोकन इंस्टॉलर में `secure-transfer` शामिल नहीं है। gzip/SHA-256 फ़ाइल स्थानांतरण, साझा PKI, सुरक्षित चैट और मेल तथा प्रमाणपत्र वाले QSL कार्ड के लिए स्रोत से बिल्ड करें। कॉन्फ़िगरेशन और हाइब्रिड चैट की शर्तें [PKI मार्गदर्शिका](../../../../../../rust/docs/pki.md) में हैं।

## इंस्टॉल या अपडेट करें

आर्किटेक्चर पता करने के लिए `uname -m` चलाएँ। डाउनलोड की निर्देशिका में नीचे के कमांड चलाएँ और उदाहरण में सही पैकेज का फ़ाइलनाम रखें।

```sh
sudo dnf install ./linkwise-modem-0.1.0-1.aarch64.rpm
linkwise-modemd --version
linkwise-modemd --simulate
```

DNF आवश्यक सिस्टम लाइब्रेरी इंस्टॉल करता है। अपडेट के लिए नई फ़ाइल के साथ `sudo dnf upgrade ./FILE.rpm` चलाएँ। सिमुलेशन बिना रेडियो के स्थानीय रूप से चलता है।

## स्टेशन सेट करें

कमांड के लिए `man 8 linkwise-modemd` और प्रारूप के लिए `man 5 linkwise-station` देखें। उपकरण चुनने और `station.json` बनाने के लिए `/usr/share/doc/linkwise-modem/Station-setup.md` खोलें। ऑडियो जाँच अपने डेस्कटॉप उपयोगकर्ता के रूप में चलाएँ, ताकि मौजूदा ऑडियो सत्र उपलब्ध रहे।

## मॉडेम सेवा चलाएँ

डेस्कटॉप स्टेशन के लिए कॉन्फ़िगरेशन `~/.config/linkwise/station.json` में सहेजें और नीचे के कमांड चलाएँ।

```sh
systemctl --user enable --now linkwise-modem.service
journalctl --user -u linkwise-modem.service
```

समर्पित स्टेशन के लिए प्रशासक के स्वामित्व वाली कॉन्फ़िगरेशन और प्रोफ़ाइल फ़ाइलें `/etc/linkwise` में रखें। सेवा को उन्हें पढ़ने और निर्धारित ALSA इनपुट तथा आउटपुट तक सीधे पहुँचने की अनुमति चाहिए। एक ही उपकरण और पोर्ट जोड़ी के लिए एक सेवा चलाएँ।

```sh
sudo systemctl enable --now linkwise-modem.service
sudo journalctl -u linkwise-modem.service
```

`bandwidth` होने पर दोनों पोर्ट IPv4 लूपबैक पर खुलते हैं। उसके बिना कमांड पोर्ट JSON में रिसेप्शन निदान देता है। उसे `nc 127.0.0.1 8300` से पढ़ें।

### SELinux

SELinux को enforcing रखें। RPM सिस्टम सेवा की नीति इंस्टॉल करता है। कॉन्फ़िगरेशन बनाने या बदलने के बाद लेबल बहाल करें और सक्रिय डोमेन जाँचें।

```sh
sudo restorecon -Rv /etc/linkwise
ls -Z /usr/bin/linkwise-modemd /etc/linkwise/station.json
ps -Z -p "$(systemctl show -p MainPID --value linkwise-modem)"
sudo ausearch -m AVC,USER_AVC -ts recent -i
```

एक्ज़ीक्यूटेबल पर `linkwise_modem_exec_t`, कॉन्फ़िगरेशन पर `linkwise_modem_etc_t` और सिस्टम सेवा पर `linkwise_modem_t` अपेक्षित हैं। सामान्य TCP 8300 और 8301 का प्रकार `linkwise_modem_port_t` है। अन्य पोर्ट पर यह प्रकार लगाएँ और पुराने लेबल टकराव सुलझाएँ। डेस्कटॉप सेवा चालू करने वाले उपयोगकर्ता के डोमेन में चलती है।

Fedora SELinux परीक्षण और DISA STIG योग्यता अलग जाँचें हैं। तैनाती से पहले STIG के दायरे की OS इमेज, नीति वेरिएंट और साइट बेसलाइन जाँचें।

## अनइंस्टॉल करें

Linkwise की चालू कमांड बंद करें और `sudo dnf remove linkwise-modem` चलाएँ। होम निर्देशिका की स्टेशन फ़ाइलें बनी रहेंगी।
