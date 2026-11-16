# macOS-এর জন্য Linkwise মডেম প্রিভিউ

Linkwise দ্বিমুখী সফটওয়্যার মডেম, স্টেশন ডায়াগনস্টিক ও স্থানীয় সিমুলেশন দেয়। অডিও রাউটিং ও প্রেরণের সেটিংয়ের জন্য Station Setup দেখুন।

প্রিভিউ ইনস্টলারে `secure-transfer` নেই। gzip/SHA-256 ফাইল স্থানান্তর, যৌথ PKI, সুরক্ষিত চ্যাট ও মেল এবং সার্টিফিকেটসহ QSL কার্ডের জন্য সোর্স থেকে বিল্ড করুন। কনফিগারেশন ও হাইব্রিড চ্যাটের শর্ত [PKI নির্দেশিকায়](../../../../../../rust/docs/pki.md) আছে।

## ইনস্টল বা আপডেট

1. Apple মেনু → About This Mac-এ চিপ বা প্রসেসর দেখুন। Apple silicon-এর জন্য `macos-arm64.pkg` বা Intel-এর জন্য `macos-x86_64.pkg` বেছে নিন।
2. Installer-এ প্যাকেজ খুলে নির্দেশনা অনুসরণ করুন। প্রশাসকের অ্যাকাউন্ট লাগে। একইভাবে নতুন প্যাকেজ চালিয়ে আপডেট করুন। স্টেশনের কনফিগারেশন নিজের হোম ডিরেক্টরিতে রাখুন।
3. Terminal খুলে নিচের কমান্ড চালান।

   ```sh
   /usr/local/bin/linkwise-modemd --version
   /usr/local/bin/linkwise-modemd --simulate
   ```

সিমুলেশন স্থানীয়ভাবে রেডিও ছাড়াই চলে। ডেভেলপমেন্ট প্যাকেজে স্বাক্ষর না-ও থাকতে পারে। macOS প্যাকেজ প্রত্যাখ্যান করলে সরবরাহকারীর কাছে স্বাক্ষরিত ও নোটারাইজ করা বিল্ড চান।

## স্টেশন সেট আপ করুন

কমান্ডের জন্য `man 8 linkwise-modemd` এবং কনফিগারেশন বিন্যাসের জন্য `man 5 linkwise-station` চালান। ডিভাইস বেছে নিতে ও `station.json` তৈরি করতে `/usr/local/share/doc/linkwise-modem/Station-setup.md` খুলুন। ক্যাপচার বাতিল হলে System Settings → Privacy & Security → Microphone-এ Terminal-এর অনুমতি দেখুন।

## মডেম লগইন এজেন্ট চালান

macOS 13 বা পরের সংস্করণে স্টেশনের কনফিগারেশন `~/Library/Application Support/Linkwise/station.json`-এ সেভ করে নিচের কমান্ড চালান।

```sh
"/Applications/Linkwise Station.app/Contents/MacOS/LinkwiseStation" enable
```

জানতে চাইলে মাইক্রোফোনের অনুমতি দিন এবং System Settings-এ ব্যাকগ্রাউন্ড আইটেম অনুমোদন করুন। নিবন্ধন দেখতে একই কমান্ডে `status` দিন। আনইনস্টল করার আগে `disable` দিয়ে নিবন্ধন বাতিল করুন।

## আনইনস্টল করুন

চলমান Linkwise কমান্ড বন্ধ করুন, তারপর ইনস্টল হওয়া ফাইল ও প্যাকেজের রসিদ মুছুন।

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

হোম ডিরেক্টরিতে রাখা স্টেশনের ফাইলগুলো থাকবে।
