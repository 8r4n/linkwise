# macOS के लिए Linkwise मॉडेम का पूर्वावलोकन

Linkwise दोतरफ़ा सॉफ़्टवेयर मॉडेम, स्टेशन निदान और स्थानीय सिमुलेशन देता है। ऑडियो रूटिंग और प्रसारण सेटिंग के लिए Station Setup देखें।

इस पूर्वावलोकन इंस्टॉलर में `secure-transfer` शामिल नहीं है। gzip/SHA-256 फ़ाइल स्थानांतरण, साझा PKI, सुरक्षित चैट और मेल तथा प्रमाणपत्र वाले QSL कार्ड के लिए स्रोत से बिल्ड करें। कॉन्फ़िगरेशन और हाइब्रिड चैट की शर्तें [PKI मार्गदर्शिका](../../../../../../rust/docs/pki.md) में हैं।

## इंस्टॉल या अपडेट करें

1. Apple मेनू → About This Mac में चिप या प्रोसेसर देखें। Apple silicon के लिए `macos-arm64.pkg` और Intel के लिए `macos-x86_64.pkg` चुनें।
2. Installer में पैकेज खोलें और निर्देशों का पालन करें। प्रशासक खाता चाहिए। अपडेट के लिए नया पैकेज इसी प्रकार चलाएँ। स्टेशन कॉन्फ़िगरेशन अपनी होम निर्देशिका में रखें।
3. Terminal खोलें और ये कमांड चलाएँ।

   ```sh
   /usr/local/bin/linkwise-modemd --version
   /usr/local/bin/linkwise-modemd --simulate
   ```

सिमुलेशन बिना रेडियो के स्थानीय रूप से चलता है। विकास पैकेज बिना हस्ताक्षर के हो सकते हैं। macOS पैकेज अस्वीकार करे तो वितरक से हस्ताक्षरित और नोटरीकृत बिल्ड माँगें।

## स्टेशन सेट करें

कमांड के लिए `man 8 linkwise-modemd` और कॉन्फ़िगरेशन प्रारूप के लिए `man 5 linkwise-station` चलाएँ। उपकरण चुनने और `station.json` बनाने के लिए `/usr/local/share/doc/linkwise-modem/Station-setup.md` खोलें। कैप्चर अस्वीकृत हो तो System Settings → Privacy & Security → Microphone में Terminal की अनुमति जाँचें।

## मॉडेम लॉगिन एजेंट चलाएँ

macOS 13 या बाद के संस्करण में स्टेशन का कॉन्फ़िगरेशन `~/Library/Application Support/Linkwise/station.json` में सहेजें और यह कमांड चलाएँ।

```sh
"/Applications/Linkwise Station.app/Contents/MacOS/LinkwiseStation" enable
```

पूछे जाने पर माइक्रोफ़ोन की अनुमति दें और System Settings में बैकग्राउंड आइटम मंज़ूर करें। पंजीकरण देखने के लिए उसी कमांड के साथ `status` दें या अनइंस्टॉल करने से पहले पंजीकरण हटाने के लिए `disable` दें।

## अनइंस्टॉल करें

चल रही Linkwise कमांड बंद करें, फिर इंस्टॉल की गई फ़ाइलें और पैकेज रसीद हटाएँ।

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

होम निर्देशिका में सहेजी गई स्टेशन फ़ाइलें बनी रहेंगी।
