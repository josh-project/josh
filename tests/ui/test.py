from selenium import webdriver
from selenium.webdriver.common.by import By
from selenium.webdriver.common.keys import Keys
from selenium.webdriver.common.action_chains import ActionChains
from selenium.webdriver.support import expected_conditions
from selenium.webdriver.support.wait import WebDriverWait
from selenium.webdriver.support.ui import WebDriverWait
from selenium.webdriver.support.expected_conditions import presence_of_element_located
from selenium.webdriver.common.desired_capabilities import DesiredCapabilities
from selenium.webdriver.firefox.options import Options
from selenium.webdriver.support import expected_conditions as EC

options = Options()
options.headless = True
options.log.level = "trace"

with webdriver.Firefox(options=options) as driver:
    wait = WebDriverWait(driver, 60)
    driver.get("http://localhost:8002/~/browse/real_repo@refs/heads/master(:/)/[]")
    wait.until(presence_of_element_located((By.CSS_SELECTOR, "#repo")))
    print(driver.find_element(By.CSS_SELECTOR, "#repo").text)
    wait.until(presence_of_element_located((By.LINK_TEXT, ":/")))
    driver.find_element(By.LINK_TEXT, ":/").click()
    wait.until(presence_of_element_located((By.CSS_SELECTOR, "td")))
    driver.find_element(By.CSS_SELECTOR, "a:nth-child(2) td").click()
    print("success")
